import AppKit
import Foundation
import SwiftUI

enum HideTheme {
    enum GitIcon {
        static let refresh = "arrow.clockwise"
        static let merged = "checkmark.circle"
        static let unmerged = "circle"
        static let dirty = "circle.fill"
        static let clean = "checkmark"
        static let pullMerged = "arrow.triangle.merge"
        static let pullClosed = "xmark.circle"
        static let unavailable = "exclamationmark.circle"
        static let noPullRequest = "minus.circle"
    }
    static let gitSectionIcon = "externaldrive.badge.checkmark"
    static let gitPullRequestIcon = "arrow.triangle.pull"
    static let lineageIndent: CGFloat = 12
    static let lineageDeepIndent: CGFloat = 6
    static let lineageChevronWidth: CGFloat = 16
    static let worktreeDialogWidth: CGFloat = 440
    static let gitRowFontSize: CGFloat = 11
    static let gitDetailFontSize: CGFloat = 10

    static let background = Color(red: 0.035, green: 0.043, blue: 0.055)
    static let sidebar = Color(red: 0.055, green: 0.063, blue: 0.078)
    static let panel = Color(red: 0.070, green: 0.080, blue: 0.098)
    static let elevated = Color(red: 0.105, green: 0.118, blue: 0.142)
    static let divider = Color.white.opacity(0.09)
    static let primary = Color.white.opacity(0.92)
    static let secondary = Color.white.opacity(0.52)
    static let muted = Color.white.opacity(0.32)
    /// File-row icons are category illustration, so their neutrals sit on this
    /// system's own neutral ladder rather than on Seti's. Seti's own neutral,
    /// `#6D8086`, is a dark slate that reads as a speck against the panel at
    /// 12px - which is what made `.gitignore` and `Cargo.toml` look unrendered.
    /// These two are DESIGN.md's `mute` and `charcoal` steps.
    static let fileIconNeutralHex = "#9C9C9D"
    static let fileIconDocumentHex = "#D3D3D4"
    /// Monospaced content sizes at a pane's default scale. The per-pane zoom
    /// chords multiply these; they are tokens rather than call-site literals so
    /// the two content surfaces cannot drift apart.
    static let terminalBaseFontSize: CGFloat = 14
    static let editorBaseFontSize: CGFloat = 12
    static let accent = Color(red: 0.725, green: 1.0, blue: 0.40)
    static let danger = Color(red: 1.0, green: 0.35, blue: 0.36)
    static let warning = Color(red: 1.0, green: 0.72, blue: 0.28)
    static let success = Color(red: 0.37, green: 0.90, blue: 0.62)
    /// How far a status mark is dimmed once the operator has read it. The mark
    /// keeps its shape and its hue so the row still says what it is; only its
    /// urgency drops (DESIGN.md, R5).
    static let readStatusOpacity: Double = 0.55
    /// The column an agent's status mark sits in. Fixed, so `?` `!` `×` and
    /// `~` line up down a list instead of shifting each row's text.
    static let agentMarkWidth: CGFloat = 12
    /// Diff line tints, named here so the changes view and any later diff
    /// surface cannot drift apart. They lean on the semantic pair above
    /// rather than introducing hues of their own.
    static let diffAdded = success
    static let diffRemoved = danger
    static let diffAddedBackground = success.opacity(0.10)
    static let diffRemovedBackground = danger.opacity(0.10)
    /// The fill behind a search match that is not the current one. Content
    /// emphasis rather than chrome, so it is allowed a saturated tint.
    static let searchMatchHighlight = accent.opacity(0.24)

    /// The same tokens as `NSColor`, for the AppKit views the shell hosts.
    /// They are converted here rather than at each call site so a view and its
    /// SwiftUI neighbours cannot end up on different values.
    enum Native {
        static let panel = NSColor(HideTheme.panel)
        static let elevated = NSColor(HideTheme.elevated)
        static let divider = NSColor(HideTheme.divider)
        static let primary = NSColor(HideTheme.primary)
        static let secondary = NSColor(HideTheme.secondary)
        static let searchMatchHighlight = NSColor(HideTheme.searchMatchHighlight)
    }

    static let spacingNone: CGFloat = 0
    static let spacingXXS: CGFloat = 2
    static let spacingXS: CGFloat = 4
    static let spacingSM: CGFloat = 8
    static let spacingMD: CGFloat = 12
    static let spacingLG: CGFloat = 16
    static let spacingXL: CGFloat = 24

    static let radiusExtraSmall: CGFloat = 4
    static let radiusSmall: CGFloat = 6
    static let radiusMedium: CGFloat = 8
    static let radiusLarge: CGFloat = 10
    static let radiusExtraLarge: CGFloat = 16

    static let compactControlSize: CGFloat = 36
    static let searchSheetSize = CGSize(width: 570, height: 430)
    static let settingsSheetSize = CGSize(width: 720, height: 560)
    static let addDeviceSheetSize = CGSize(width: 470, height: 300)

    /// Sizes that describe the window's three-column frame rather than the
    /// spacing and radius scale above, which any view may reach for.
    enum Layout {
        static let hairlineWidth: CGFloat = 1
        static let resizeHandleThickness: CGFloat = 2
        /// The strip that answers the pointer. Wider than the 2pt marker it
        /// draws, because a divider has to be easy to grab, not easy to see.
        static let resizeHandleGrabWidth: CGFloat = 20
        static let panelCollapseControlSize: CGFloat = 18
        /// How far a press has to travel on a tab before it is a reorder
        /// rather than a click. Below this a tremor while selecting a tab
        /// would carry it out of its slot.
        static let tabDragActivationDistance: CGFloat = 6
        /// The window's first row. A tab, the new-tab control, and the strip
        /// itself are all this tall, so the row cannot grow taller than the
        /// thing inside it.
        static let tabStripHeight: CGFloat = 32
        /// How much of the window's first row the traffic lights own. They end
        /// 61pt from the left edge, the zoom button spanning 47 to 61, so
        /// whichever surface reaches that corner keeps this much clear: the
        /// measurement plus one spacing step.
        static let trafficLightInset: CGFloat = 69
        static let paneHeaderHeight: CGFloat = 28
        static let sidebarMinWidth: CGFloat = 220
        static let sidebarIdealWidth: CGFloat = 292
        static let sidebarMaxWidth: CGFloat = 440
        static let terminalMinWidth: CGFloat = 540
        static let terminalIdealWidth: CGFloat = 760
        static let rightPanelMinWidth: CGFloat = 260
        static let rightPanelIdealWidth: CGFloat = 355
        static let rightPanelMaxWidth: CGFloat = 560
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

/// Whether the canvas a view sits on is the one on top. A retained canvas
/// for a tab that is not showing is kept in the tree for its scrollback,
/// and the terminal views on it read this to stop drawing while hidden.
private struct HideCanvasVisibleKey: EnvironmentKey {
    static let defaultValue = true
}

extension EnvironmentValues {
    var hideAccent: Color {
        get { self[HideAccentKey.self] }
        set { self[HideAccentKey.self] = newValue }
    }

    var hideCanvasVisible: Bool {
        get { self[HideCanvasVisibleKey.self] }
        set { self[HideCanvasVisibleKey.self] = newValue }
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
                        .drawsToWindowTopEdge()
                        .frame(
                            minWidth: HideTheme.Layout.sidebarMinWidth,
                            idealWidth: HideTheme.Layout.sidebarIdealWidth,
                            maxWidth: HideTheme.Layout.sidebarMaxWidth
                        )
                        .frame(maxHeight: .infinity, alignment: .topLeading)
                }
                HideMainView()
                    .drawsToWindowTopEdge()
                    .frame(
                        minWidth: HideTheme.Layout.terminalMinWidth,
                        idealWidth: HideTheme.Layout.terminalIdealWidth,
                        maxWidth: .infinity,
                        maxHeight: .infinity,
                        alignment: .topLeading
                    )
                if model.rightPanelVisible {
                    RightPanel()
                        .drawsToWindowTopEdge()
                        .frame(
                            minWidth: HideTheme.Layout.rightPanelMinWidth,
                            idealWidth: HideTheme.Layout.rightPanelIdealWidth,
                            maxWidth: HideTheme.Layout.rightPanelMaxWidth,
                            maxHeight: .infinity
                        )
                        .accessibilityIdentifier("right-panel")
                }
            }
            if let cycle = model.agentSwitcherCycle {
                AgentSwitcherOverlay(cycle: cycle, agents: model.agents)
            } else if let cycle = model.tabSwitcherCycle {
                TabSwitcherOverlay(
                    cycle: cycle,
                    tabs: model.unifiedTabs,
                    checkoutLabel: model.focusedCheckout?.label
                )
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(HideTheme.background)
        .preferredColorScheme(.dark)
        .environment(\.colorScheme, .dark)
        .environment(\.hideAccent, HideTheme.color(for: model.core.snapshot?.uiState.accentHex ?? "#B9FF66"))
        .environment(\.hideFontScale, CGFloat((model.core.snapshot?.uiState.fontSize ?? 13) / 13))
        .tint(HideTheme.color(for: model.core.snapshot?.uiState.accentHex ?? "#B9FF66"))
        .sheet(isPresented: $model.showNewAgent) {
            NewAgentSheet()
                .environmentObject(model)
        }
        .sheet(isPresented: $model.showSearch) {
            HideSearchSheet()
                .environmentObject(model)
        }
        .sheet(isPresented: $model.showFileSearch) {
            WorkspaceFileSearchSheet()
                .environmentObject(model)
        }
        .sheet(isPresented: $model.showSettings) {
            HideSettingsView(model: model, showsCloseButton: true)
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
        .sheet(item: $model.worktreeToDelete) { worktree in
            VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
                Text("Delete worktree \(worktree.label)?")
                    .hideFont(size: HideTheme.gitRowFontSize, weight: .semibold)
                Text(worktree.deletionConsequence)
                    .hideFont(size: HideTheme.gitRowFontSize)
                    .foregroundStyle(HideTheme.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if worktree.deletionGate.canDeleteBranch {
                    Toggle("Also delete local branch \(worktree.branch ?? "")", isOn: $model.deleteWorktreeBranch)
                        .toggleStyle(.checkbox)
                }
                HStack {
                    Spacer()
                    Button("Cancel") { model.worktreeToDelete = nil }.keyboardShortcut(.cancelAction)
                    Button(worktree.deletionGate.buttonLabel, role: .destructive, action: model.confirmDeleteWorktree)
                }
            }
            .padding(HideTheme.spacingXL)
            .frame(width: HideTheme.worktreeDialogWidth)
            .background(HideTheme.panel)
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
                            Text("\(agent.contextLabel) · \(paneID)")
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

private struct TabSwitcherOverlay: View {
    let cycle: TabSwitcherCycle
    let tabs: [ShellTabItem]
    let checkoutLabel: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("RECENT TABS")
                .hideFont(size: 10, weight: .bold)
                .foregroundStyle(HideTheme.muted)
            ForEach(cycle.tabIDs, id: \.self) { tabID in
                if let tab = tabs.first(where: { $0.id == tabID }) {
                    HStack(spacing: 10) {
                        Image(systemName: icon(for: tab))
                            .hideFont(size: 13, weight: .semibold)
                            .foregroundStyle(HideTheme.secondary)
                            .frame(width: 19, height: 19)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(tab.label)
                                .hideFont(size: 12, weight: .semibold)
                                .lineLimit(1)
                            Text(detail(for: tab))
                                .hideFont(size: 9, design: .monospaced)
                                .foregroundStyle(HideTheme.secondary)
                                .lineLimit(1)
                        }
                        Spacer()
                        if tab.dirty {
                            Circle()
                                .fill(HideTheme.secondary)
                                .frame(width: 5, height: 5)
                        }
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 44)
                    .background(
                        tabID == cycle.selectedTabID ? HideTheme.accent.opacity(0.16) : Color.clear,
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
        .accessibilityIdentifier("tab-mru-switcher")
    }

    private func icon(for tab: ShellTabItem) -> String {
        switch tab.kind {
        case .herdr: "terminal"
        case .file: "doc.text"
        }
    }

    private func detail(for tab: ShellTabItem) -> String {
        let kind = switch tab.kind {
        case .herdr: "Terminal"
        case .file: "File"
        }
        guard let checkoutLabel, !checkoutLabel.isEmpty else { return kind }
        return "\(kind) · \(checkoutLabel)"
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
                PetCountTile(
                    label: AgentGroup.needsYou.title.uppercased(),
                    value: projection.counts.needsYou,
                    color: HideTheme.warning
                )
                PetCountTile(
                    label: AgentGroup.done.title.uppercased(),
                    value: projection.counts.done,
                    color: HideTheme.success
                )
                PetCountTile(
                    label: AgentGroup.working.title.uppercased(),
                    value: projection.counts.working,
                    color: HideTheme.accent
                )
                PetCountTile(
                    label: AgentGroup.seen.title.uppercased(),
                    value: projection.counts.seen,
                    color: HideTheme.secondary
                )
                // The warning hue, the same one a disconnected row carries in
                // `AgentStatusStyle`, so the tile and the row cannot say a
                // different thing about the same state.
                PetCountTile(
                    label: "DISCONNECTED",
                    value: projection.counts.disconnected,
                    color: HideTheme.warning
                )
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

/// A pet dashboard row: the one agent row on this surface's own card. What the
/// row says is the row's business, so the dashboard adds only the card.
private struct PetDashboardAgentRow: View {
    @Environment(\.hideAccent) private var accent
    let agent: PetDashboardRow
    let action: () -> Void

    var body: some View {
        AgentRow(
            presentation: AgentRowPresentation(row: agent, accent: accent),
            action: action
        )
        .frame(minHeight: 56)
        .background(
            HideTheme.elevated.opacity(0.75),
            in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
        )
        .accessibilityIdentifier("pet-dashboard-agent-\(agent.paneID)")
    }
}

private struct HideSidebar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HideBrandHeader()
            SidebarContentPicker()
            SidebarCommandBar()

            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    switch model.sidebarContent {
                    case .projects:
                        projectsContent
                    case .agents:
                        agentsContent
                    }
                }
                .padding(.bottom, 14)
            }

            SidebarUtilityBar(
                devices: model.devices,
                usages: model.core.snapshot?.navigator.providerUsage ?? []
            )
        }
        .background(HideTheme.sidebar)
        .overlay(alignment: .trailing) {
            Rectangle()
                .fill(HideTheme.divider)
                .frame(width: 1)
        }
        .accessibilityIdentifier("hide-sidebar")
    }

    @ViewBuilder
    private var projectsContent: some View {
        // What is waiting, then what finished while the operator was away,
        // come before where things live: they are the only parts that ask for
        // an action. An empty group is not drawn at all.
        ForEach(model.raisedAgentSections) { section in
            HideSectionLabel(title: section.group.title, count: section.agents.count)
            ForEach(section.agents) { agent in
                AgentNavigatorRow(agent: agent, showsWorkspace: true)
            }
        }

        HideSectionLabel(title: "Projects", count: model.workspaces.count)
        if model.workspaces.isEmpty {
            EmptySidebarRow(
                systemImage: "square.stack.3d.up",
                title: "No projects yet",
                detail: "Add a folder to create your first project."
            )
        } else {
            ForEach(model.workspaces) { workspace in
                WorkspaceNavigatorRow(workspace: workspace)
            }
        }
    }

    @ViewBuilder
    private var agentsContent: some View {
        if model.agents.isEmpty {
            HideSectionLabel(title: "Agents", count: 0)
            EmptySidebarRow(
                systemImage: "person.2",
                title: "No agents running",
                detail: "Start an agent from a project to see it here."
            )
        } else {
            // The four group boundaries, in the order the core sorted them.
            // Membership and order are the core's answer; this only draws it.
            ForEach(model.agentSections) { section in
                HideSectionLabel(title: section.group.title, count: section.agents.count)
                ForEach(section.agents) { agent in
                    AgentNavigatorRow(agent: agent, showsWorkspace: true)
                }
            }
        }
    }
}

private struct SidebarContentPicker: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: 2) {
            ForEach(SidebarContent.allCases) { content in
                Button {
                    model.showSidebarContent(content)
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: content.systemImage)
                            .hideFont(size: 10, weight: .semibold)
                        Text(content.title)
                            .hideFont(size: 10, weight: .semibold)
                    }
                    .foregroundStyle(
                        model.sidebarContent == content ? HideTheme.primary : HideTheme.muted
                    )
                    .frame(maxWidth: .infinity, minHeight: 26)
                    .background(
                        model.sidebarContent == content ? HideTheme.elevated : Color.clear,
                        in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    )
                }
                .buttonStyle(.plain)
                .help("Show \(content.title) (⌘E)")
                .accessibilityIdentifier("hide-sidebar-view-\(content.rawValue)")
            }
        }
        .padding(3)
        .background(HideTheme.panel, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
        .overlay {
            RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 8)
        .accessibilityIdentifier("hide-sidebar-view-switcher")
    }
}

private struct SidebarCommandBar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: 6) {
            Button(action: model.openSearch) {
                HStack(spacing: 7) {
                    Image(systemName: "magnifyingglass")
                        .hideFont(size: 11, weight: .semibold)
                    Text("Search")
                        .hideFont(size: 11, weight: .medium)
                    Spacer(minLength: 4)
                    Text("⌘K")
                        .hideFont(size: 9, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                }
                .foregroundStyle(HideTheme.secondary)
                .padding(.horizontal, 10)
                .frame(maxWidth: .infinity, minHeight: 32)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Search projects and agents")

            SidebarIconButton(
                systemImage: "folder.badge.plus",
                help: "New project (⌘⇧N)",
                accessibilityLabel: "New project",
                action: model.openNewWorkspace
            )
            SidebarIconButton(
                systemImage: "plus",
                help: "New agent (⌘N)",
                accessibilityLabel: "New agent",
                action: { model.openNewAgent() }
            )
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 10)
    }
}

private struct SidebarIconButton: View {
    let systemImage: String
    let help: String
    let accessibilityLabel: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .hideFont(size: 11, weight: .semibold)
                .foregroundStyle(HideTheme.secondary)
                .frame(width: 32, height: 32)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
        }
        .buttonStyle(.plain)
        .help(help)
        .accessibilityLabel(accessibilityLabel)
    }
}

private struct SidebarUtilityBar: View {
    @EnvironmentObject private var model: ShellModel
    @State private var showingUsage = false

    let devices: [CoreDeviceSnapshot]
    let usages: [CoreProviderUsageSnapshot]

    private var selectedDevice: CoreDeviceSnapshot? {
        devices.first(where: { $0.id == model.selectedDeviceID }) ?? devices.first
    }

    private func availablePercent(for usage: CoreProviderUsageSnapshot) -> Double? {
        guard usage.state == "available" else { return nil }
        return usage.usedPercent
    }

    private func usageColor(for usage: CoreProviderUsageSnapshot) -> Color {
        guard let percent = availablePercent(for: usage) else { return HideTheme.muted }
        if percent >= 90 { return HideTheme.danger }
        if percent >= 70 { return HideTheme.warning }
        return HideTheme.success
    }

    var body: some View {
        HStack(spacing: 5) {
            Menu {
                ForEach(devices) { device in
                    Button {
                        model.selectDevice(device)
                    } label: {
                        Label(
                            device.agentCount > 0
                                ? "\(device.label), \(device.agentCount) agents"
                                : device.label,
                            systemImage: device.id == model.selectedDeviceID ? "checkmark" : "circle"
                        )
                    }
                }
            } label: {
                HStack(spacing: 7) {
                    Circle()
                        .fill(selectedDevice.map(deviceStatusColor) ?? HideTheme.muted)
                        .frame(width: 6, height: 6)
                    Text(selectedDevice?.label ?? "No device")
                        .hideFont(size: 10, weight: .medium)
                        .lineLimit(1)
                    if let agentCount = selectedDevice?.agentCount, agentCount > 0 {
                        Text("\(agentCount)")
                            .hideFont(size: 9, design: .monospaced)
                            .foregroundStyle(HideTheme.muted)
                    }
                    Image(systemName: "chevron.up.chevron.down")
                        .hideFont(size: 8, weight: .semibold)
                        .foregroundStyle(HideTheme.muted)
                }
                .foregroundStyle(HideTheme.secondary)
                .padding(.horizontal, 9)
                .frame(height: 30)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("Choose device")

            Spacer(minLength: 0)

            Button {
                showingUsage.toggle()
            } label: {
                HStack(spacing: HideTheme.spacingXS) {
                    ForEach(usages) { usage in
                        HStack(spacing: HideTheme.spacingXXS) {
                            HideProviderMark(
                                usage: usage,
                                isMuted: availablePercent(for: usage) == nil
                            )
                            if let percent = availablePercent(for: usage) {
                                Text("\(Int(percent.rounded()))%")
                                    .hideFont(size: 9, weight: .semibold, design: .monospaced)
                            }
                        }
                        .foregroundStyle(usageColor(for: usage))
                        .padding(.horizontal, HideTheme.spacingXS)
                        .frame(minHeight: 30)
                        .background(
                            HideTheme.elevated,
                            in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        )
                    }
                }
                .frame(minWidth: 30, minHeight: 30)
            }
            .buttonStyle(.plain)
            .help("Weekly provider usage")
            .accessibilityLabel("Weekly provider usage")
            .popover(isPresented: $showingUsage, arrowEdge: .bottom) {
                HideUsagePopover(usages: usages)
            }

            Button {
                model.showSettings = true
            } label: {
                Image(systemName: "gearshape")
                    .hideFont(size: 11, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                    .frame(width: 30, height: 30)
            }
            .buttonStyle(.plain)
            .help("Settings")
            .accessibilityLabel("Settings")
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .overlay(alignment: .top) {
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: HideTheme.Layout.hairlineWidth)
        }
    }

    private func deviceStatusColor(_ device: CoreDeviceSnapshot) -> Color {
        device.state == "ready" || device.state == "available"
            ? HideTheme.success
            : HideTheme.warning
    }
}

private struct HideProviderMark: View {
    let usage: CoreProviderUsageSnapshot
    let isMuted: Bool

    var body: some View {
        Group {
            if let mark = AgentMark.image(for: usage.provider) {
                Image(nsImage: mark)
                    .renderingMode(isMuted ? .template : .original)
                    .resizable()
                    .interpolation(.high)
                    .aspectRatio(contentMode: .fit)
                    .foregroundStyle(HideTheme.muted)
                    .padding(HideTheme.spacingXXS)
            } else {
                Text(usage.label.prefix(1))
                    .hideFont(size: 9, weight: .bold, design: .rounded)
                    .foregroundStyle(isMuted ? HideTheme.muted : HideTheme.secondary)
            }
        }
        .frame(width: HideTheme.spacingLG, height: HideTheme.spacingLG)
        .background(
            HideTheme.elevated,
            in: RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
        )
    }
}

private struct HideUsagePopover: View {
    let usages: [CoreProviderUsageSnapshot]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 7) {
                Text("Weekly Usage")
                    .hideFont(size: 12, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                Spacer()
                Text("7 days")
                    .hideFont(size: 9, weight: .semibold, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }

            if usages.isEmpty {
                Text("Provider usage is not available yet.")
                    .hideFont(size: 10)
                    .foregroundStyle(HideTheme.secondary)
            } else {
                ForEach(usages) { usage in
                    HideWeeklyUsageRow(usage: usage)
                }
            }
        }
        .padding(14)
        .frame(width: 250)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .accessibilityIdentifier("hide-weekly-usage")
    }
}

private struct HideWeeklyUsageRow: View {
    let usage: CoreProviderUsageSnapshot

    private var clampedProgress: Double {
        min(max(usage.usedPercent ?? 0, 0), 100) / 100
    }

    private var usageColor: Color {
        guard let percent = usage.usedPercent, usage.state == "available" else {
            return HideTheme.muted
        }
        if percent >= 90 { return HideTheme.danger }
        if percent >= 70 { return HideTheme.warning }
        return HideTheme.success
    }

    private var valueLabel: String {
        guard let percent = usage.usedPercent, usage.state == "available" else {
            return "Unavailable"
        }
        return "\(Int(percent.rounded()))%"
    }

    private var helpText: String {
        if let message = usage.message {
            return message
        }
        guard let reset = usage.resetsAtUnixSeconds else {
            return "1-week plan usage"
        }
        let date = Date(timeIntervalSince1970: TimeInterval(reset))
        return "Resets \(date.formatted(date: .abbreviated, time: .shortened))"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 7) {
                HideProviderMark(usage: usage, isMuted: false)

                Text(usage.label)
                    .hideFont(size: 11, weight: .medium)
                    .foregroundStyle(HideTheme.secondary)
                    .lineLimit(1)
                Spacer(minLength: 8)
                Text(valueLabel)
                    .hideFont(size: 10, weight: .semibold, design: .monospaced)
                    .foregroundStyle(usage.state == "available" ? usageColor : HideTheme.muted)
            }

            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(HideTheme.divider)
                    RoundedRectangle(cornerRadius: 2)
                        .fill(usageColor)
                        .frame(width: geometry.size.width * clampedProgress)
                }
            }
            .frame(height: 3)
            .accessibilityHidden(true)
        }
        .help(helpText)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(usage.label)
        .accessibilityValue(valueLabel)
        .accessibilityIdentifier("hide-weekly-usage-\(usage.provider)")
    }
}

private struct HideBrandHeader: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Text("hide")
                .hideFont(size: 18, weight: .bold, design: .rounded)
                .tracking(-0.6)
                .foregroundStyle(HideTheme.primary)
            Circle()
                .fill(model.herdrIsConnected ? HideTheme.success : HideTheme.warning)
                .frame(width: 6, height: 6)
            Spacer()
            Text(model.isRemoteContext ? model.remote.targetLabel : (model.core.runtimeSelection?.version ?? "offline"))
                .hideFont(size: 9, weight: .medium, design: .monospaced)
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
        // The wordmark starts after the traffic lights rather than under them.
        .padding(.leading, HideTheme.Layout.trafficLightInset)
        .padding(.trailing, HideTheme.spacingMD)
        .padding(.vertical, HideTheme.spacingMD)
        // This is the window's top left corner while the sidebar is open, and
        // with the titlebar gone it is where the window is grabbed. The
        // sidebar toggle takes its own clicks; everything else here is the
        // handle.
        .background(WindowDragArea())
        .accessibilityIdentifier("hide-brand")
    }
}

private struct HideSectionLabel: View {
    let title: String
    let count: Int?

    var body: some View {
        HStack(spacing: 7) {
            Text(title)
                .hideFont(size: 11, weight: .semibold)
                .foregroundStyle(HideTheme.secondary)
            if let count {
                Text("\(count)")
                    .hideFont(size: 9, weight: .medium, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.top, 13)
        .padding(.bottom, 6)
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

    private var isFocusedWorkspace: Bool {
        model.focusedWorkspace?.id == workspace.id
    }

    private var presentation: SidebarWorkspacePresentation {
        SidebarWorkspacePresentation(workspace: workspace, agents: model.agents)
    }

    /// The rows the Projects view already drew above the tree. Drawing one
    /// again under its checkout would say the same thing twice.
    private var raisedAgentIDs: Set<String> {
        Set(model.raisedAgents.map(\.id))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            HStack(spacing: 0) {
                Button {
                    model.toggleWorkspace(workspace)
                } label: {
                    HStack(spacing: 7) {
                        Image(systemName: workspace.expanded ? "chevron.down" : "chevron.right")
                            .hideFont(size: 9, weight: .bold)
                            .foregroundStyle(HideTheme.muted)
                            .frame(width: 12, height: 20)
                        Image(systemName: workspace.isGit ? "folder.badge.gearshape" : "folder")
                            .hideFont(size: 12, weight: .semibold)
                            .foregroundStyle(
                                workspace.temporary
                                    ? HideTheme.warning
                                    : (isFocusedWorkspace ? accent : HideTheme.secondary)
                            )
                            .frame(width: 16)
                        Text(workspace.label)
                            .hideFont(size: 12, weight: .semibold)
                            .foregroundStyle(isFocusedWorkspace ? HideTheme.primary : HideTheme.secondary)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        Text(presentation.activityLabel)
                            .hideFont(size: 9, design: .monospaced)
                            .foregroundStyle(HideTheme.muted)
                            .lineLimit(1)
                    }
                    .frame(maxWidth: .infinity, minHeight: 34)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(workspace.expanded ? "Collapse \(workspace.label)" : "Expand \(workspace.label)")
                .accessibilityIdentifier("hide-workspace-disclosure-\(workspace.id)")
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
                .menuIndicator(.hidden)
                .fixedSize()
                .frame(width: 24, height: 28)
            }
            .padding(.leading, 10)
            .padding(.trailing, 8)

            if workspace.expanded {
                ForEach(workspace.checkouts) { checkout in
                    checkoutGroup(checkout)
                }
            }
        }
        .padding(.bottom, 3)
    }

    private func checkoutGroup(_ checkout: CoreCheckoutSnapshot) -> some View {
        let isFocused = model.focusedCheckout?.id == checkout.id
        let visibleAgents = SidebarGrouping.tree(model.agents, checkoutID: checkout.id, excluding: raisedAgentIDs)
        let checkoutPresentation = SidebarCheckoutPresentation(
            workspace: workspace,
            checkout: checkout,
            agents: model.agents
        )

        return VStack(alignment: .leading, spacing: 0) {
            CheckoutNavigatorRow(
                workspace: workspace,
                checkout: checkout,
                presentation: checkoutPresentation,
                isFocused: isFocused
            )
            ForEach(visibleAgents) { agent in
                AgentNavigatorRow(agent: agent, showsWorkspace: false)
            }
        }
        .background(
            isFocused ? HideTheme.elevated.opacity(0.72) : .clear,
            in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
        )
        .overlay {
            if isFocused {
                RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
        }
        .padding(.horizontal, 8)
    }
}

private struct CheckoutNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent
    let workspace: CoreWorkspaceSnapshot
    let checkout: CoreCheckoutSnapshot
    let presentation: SidebarCheckoutPresentation
    let isFocused: Bool

    private var activityColor: Color {
        switch presentation.activity {
        case .missing, .error: HideTheme.danger
        case .needsAttention: HideTheme.warning
        case .working: accent
        case .idle: HideTheme.secondary
        case .empty: HideTheme.muted
        }
    }

    var body: some View {
        Button {
            model.selectCheckout(checkout)
        } label: {
            HStack(spacing: 7) {
                Circle()
                    .fill(activityColor)
                    .frame(width: 6, height: 6)
                Image(systemName: checkout.isWorktree ? "arrow.triangle.branch" : "rectangle.stack")
                    .hideFont(size: 10, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                    .frame(width: 14)
                Text(checkout.label)
                    .hideFont(size: 11, weight: isFocused ? .semibold : .regular)
                    .foregroundStyle(isFocused ? HideTheme.primary : HideTheme.secondary)
                    .lineLimit(1)
                if !checkout.exists {
                    SidebarBadge(label: "missing", color: HideTheme.danger)
                } else if checkout.temporary {
                    SidebarBadge(label: "temporary", color: HideTheme.warning)
                } else if presentation.isPrimary {
                    SidebarBadge(label: "main worktree", color: HideTheme.secondary)
                }
                // The three things a row may say about a worktree, and no
                // more: what its pull request is, that something is
                // uncommitted, and how many agents are in it (R2, G1).
                if let pullRequest = checkout.pullRequest {
                    SidebarBadge(
                        label: CheckoutCardPresentation.badgeLabel(
                            pullRequest.badge,
                            review: pullRequest.review
                        ),
                        color: CheckoutCardPresentation.badgeColor(
                            pullRequest.badge,
                            review: pullRequest.review
                        )
                    )
                }
                if checkout.dirty {
                    Circle()
                        .fill(HideTheme.warning)
                        .frame(width: 5, height: 5)
                        .help("\(checkout.changedFileCount) uncommitted changes")
                }
                Spacer(minLength: 0)
                if presentation.agentCount > 0 {
                    Text("\(presentation.agentCount)")
                        .hideFont(size: 9, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                }
            }
            // A worktree with no terminal, and one whose pull request is
            // settled, are both things to look past rather than at.
            .opacity(CheckoutCardPresentation.isDimmed(checkout) ? 0.55 : 1)
            .padding(.leading, 23)
            .padding(.trailing, 9)
            .frame(minHeight: 31)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(checkout.branch.map { "\($0)\n\(checkout.path)" } ?? checkout.path)
        .accessibilityIdentifier("hide-checkout-\(checkout.id)")
        // The row is deliberately almost wordless, so everything the colours,
        // dots, and badges carry is said here in words (G1, design 7).
        .accessibilityLabel(
            CheckoutCardPresentation.rowAccessibilityLabel(
                repoName: workspace.repoName,
                checkout: checkout,
                agentCount: presentation.agentCount
            )
        )
        .accessibilityValue(isFocused ? "Selected" : "Not selected")
        .contextMenu {
            Button("Start agent here") { model.openNewAgent(checkoutID: checkout.id) }
            if checkout.isWorktree {
                Divider()
                Button(checkout.worktree?.deletionGate.buttonLabel ?? "Delete worktree…", role: .destructive) {
                    model.requestDeleteWorktree(checkout)
                }
                .disabled(checkout.worktree?.deletionGate.blockedReason != nil || checkout.worktree == nil)
                if let reason = checkout.worktree?.deletionGate.blockedReason { Text(reason) }
            }
        }
    }
}

struct SidebarBadge: View {
    let label: String
    let color: Color

    var body: some View {
        Text(label)
            .hideFont(size: 8, weight: .medium)
            .foregroundStyle(color)
            .padding(.horizontal, 5)
            .frame(height: 16)
            .background(HideTheme.panel, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
    }
}

/// A sidebar agent row: the one agent row plus the sidebar's focus state and
/// its direct-select shortcut hint.
private struct AgentNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent
    let agent: SidebarAgent
    /// Under a checkout the project name is the heading above the row, so
    /// repeating it wastes the line the summary needs.
    let showsWorkspace: Bool

    private var density: AgentRowDensity { showsWorkspace ? .prominent : .compact }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
            HStack(spacing: HideTheme.spacingNone) {
                if !showsWorkspace && !agent.lineageChildPaneIDs.isEmpty {
                    Button {
                        model.core.dispatch(kind: "agent_tree_toggle", payload: ["pane_id": agent.paneID])
                    } label: {
                        Image(systemName: agent.lineageCollapsed ? "chevron.right" : "chevron.down")
                            .foregroundStyle(HideTheme.muted)
                    }
                    .buttonStyle(.plain)
                    .frame(width: HideTheme.lineageChevronWidth)
                    .help(agent.lineageCollapsed ? "Expand descendants" : "Collapse descendants")
                }
                AgentRow(
                    presentation: AgentRowPresentation(
                        agent: agent,
                        density: density,
                        accent: accent
                    ),
                    density: density,
                    isFocused: model.focusedPaneID == agent.paneID,
                    shortcutNumber: model.agentShortcutHintsVisible
                        ? model.agentShortcutNumber(paneID: agent.paneID)
                        : nil,
                    action: { model.selectAgent(agent) }
                )
            }
            if !showsWorkspace, let badge = agent.lineageWorktreeBadge {
                SidebarBadge(label: badge, color: HideTheme.secondary)
            }
            if let hint = showsWorkspace ? agent.raisedHint : agent.lineageHint {
                Text(hint).hideFont(size: HideTheme.gitDetailFontSize).foregroundStyle(HideTheme.muted)
            }
        }
        .padding(.leading, showsWorkspace ? HideTheme.spacingNone :
            CGFloat(min(agent.lineageDepth, 2)) * HideTheme.lineageIndent +
            CGFloat(max(0, agent.lineageDepth - 2)) * HideTheme.lineageDeepIndent)

        .animation(.easeOut(duration: 0.12), value: model.agentShortcutHintsVisible)
        .accessibilityIdentifier("hide-agent-\(agent.id)")
    }
}

private struct HideMainView: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(spacing: 0) {
            HideTabStrip()
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: HideTheme.Layout.hairlineWidth)
            ZStack {
                HideTerminalSurface()
                if !model.isRemoteContext,
                   model.core.snapshot?.editor.activeTabID != nil {
                    FileViewerOverlay()
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
    /// What the pointer is carrying right now. This is the only piece of the
    /// strip the shell holds: the order itself belongs to the core, so a drop
    /// is reported rather than applied here.
    @State private var draggingTabID: String?
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

            if model.focusedWorkspace != nil {
                HStack(spacing: 0) {
                    ScrollViewReader { scroll in
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 0) {
                            ForEach(model.unifiedTabs) { tab in
                                HStack(spacing: 0) {
                                    Button {
                                        model.focusUnifiedTab(tab)
                                    } label: {
                                        HStack(spacing: HideTheme.spacingSM) {
                                            Image(systemName: tabIcon(tab))
                                                .font(.system(size: 10, weight: .medium))
                                            Text(tab.label)
                                                .hideFont(size: 10, weight: tab.active ? .semibold : .medium)
                                                .lineLimit(1)
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
                                                SidebarBadge(
                                                    label: "⌘\(shortcutNumber)",
                                                    color: HideTheme.secondary
                                                )
                                                .opacity(model.tabShortcutHintsVisible ? 1 : 0)
                                            }
                                        }
                                        .foregroundStyle(tab.active ? HideTheme.primary : HideTheme.secondary)
                                        .padding(.leading, HideTheme.spacingMD)
                                        .padding(.trailing, HideTheme.spacingSM)
                                        .frame(height: HideTheme.Layout.tabStripHeight)
                                        .contentShape(Rectangle())
                                    }
                                    .buttonStyle(.plain)

                                    Button {
                                        model.closeUnifiedTab(tab)
                                    } label: {
                                        Image(systemName: "xmark")
                                            .font(.system(size: 8, weight: .semibold))
                                            .frame(width: 20, height: 20)
                                            .contentShape(Rectangle())
                                    }
                                    .buttonStyle(.plain)
                                    .foregroundStyle(tab.active ? HideTheme.secondary : HideTheme.muted)
                                    .help("Close \(tab.label) (⌘W)")
                                    .accessibilityLabel("Close \(tab.label)")
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
                        .animation(.easeOut(duration: 0.12), value: model.tabShortcutHintsVisible)
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
                    Button {
                        model.addTab()
                    } label: {
                        Image(systemName: "plus")
                            .font(.system(size: 10, weight: .semibold))
                            .frame(
                                width: HideTheme.Layout.tabStripHeight,
                                height: HideTheme.Layout.tabStripHeight
                            )
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(HideTheme.secondary)
                    .help("New Tab (⌘T)")
                    .accessibilityLabel("New Herdr tab")
                    .accessibilityIdentifier("hide-new-tab")
                }
                // The strip takes the row before the drag area does. Sharing
                // the row equally cut the strip to four tabs while the rest
                // of the row stayed empty.
                .layoutPriority(1)
            }

            WindowDragArea()
                .frame(maxWidth: .infinity, maxHeight: .infinity)

            if !model.rightPanelVisible {
                Button {
                    model.toggleRightPanel()
                } label: {
                    Image(systemName: "rectangle.rightthird.inset.filled")
                }
                .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                .help("Show Right Panel (\(ShellMenuCommand.toggleRightPanel.displayShortcut))")
                .accessibilityLabel("Show Right Panel")
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
        case .file: "doc.text"
        }
    }
}

struct HideToolbarButtonStyle: ButtonStyle {
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
        .accessibilityIdentifier("hide-terminal-surface")
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
                        isFocused: item.isFocused, isZoomed: isZoomed,
                        onFocus: { model.focusPane(pane.id) },
                        onClose: { model.closePaneFromHeader(pane.id) }
                    )
                case .unavailable(let reason):
                    HideTerminalPaneCard(
                        paneID: pane.id, kind: "unavailable", title: pane.herdrLabel ?? "Pane unavailable",
                        status: "ready", isFocused: item.isFocused, isZoomed: isZoomed,
                        onFocus: { model.focusPane(pane.id) },
                        onClose: { model.closePaneFromHeader(pane.id) }
                    ) {
                        ContentUnavailableView("Pane unavailable", systemImage: "exclamationmark.triangle", description: Text(reason))
                    }
                case .terminal:
                    PaneTerminalCell(
                        pane: pane,
                        status: model.paneStatus(for: pane.id),
                        statusMessage: model.paneTransportMessage(for: pane.id),
                        isFocused: item.isFocused,
                        isZoomed: isZoomed,
                        showsFork: model.canForkPane(pane),
                        activity: model.paneActivity(for: pane.id),
                        notice: model.paneNotice(for: pane.id),
                        onFocus: { model.focusPane(pane.id) },
                        onReconnect: { model.reconnectPane(pane.id) },
                        onClose: { model.closePaneFromHeader(pane.id) },
                        onFork: { model.forkPaneFromHeader(pane.id) },
                        onOpenPort: { model.openPanePort($0) }
                    ) {
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
                        .accessibilityIdentifier("hide-empty-state-new-workspace")
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
                        // This state used to promise a terminal Hide never
                        // started: nothing calls `startTerminal` from here.
                        // The empty state now carries the control that starts
                        // one, and says only what pressing it does.
                        Text("No terminal open")
                            .hideFont(size: 17, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text("This checkout has no terminal pane. Start one to fill the workspace at its path.")
                            .hideFont(size: 12)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 360)
                        Button("Start new terminal") { model.addTab() }
                            .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                            .accessibilityIdentifier("hide-empty-state-start-terminal")
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
        .hideFont(size: 10, weight: .medium)
        .foregroundStyle(HideTheme.secondary)
        .padding(.horizontal, 14)
        .frame(height: 27)
        .background(HideTheme.panel)
        .accessibilityIdentifier("hide-status-bar")
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
        .frame(width: HideTheme.searchSheetSize.width, height: HideTheme.searchSheetSize.height)
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

struct SheetHeader: View {
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
