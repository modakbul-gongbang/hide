import AppKit
import Foundation
import SwiftUI

private struct HideAccentKey: EnvironmentKey {
    static let defaultValue = HideTheme.accent
}

private struct HidePetAppearanceKey: EnvironmentKey {
    static let defaultValue = false
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
    var hidePetAppearance: Bool {
        get { self[HidePetAppearanceKey.self] }
        set { self[HidePetAppearanceKey.self] = newValue }
    }

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
    @Environment(\.hidePetAppearance) private var petAppearance

    func body(content: Content) -> some View {
        content.font(petAppearance
            ? .system(size: size * scale, weight: weight, design: design)
            : HideTheme.font(size: size * scale, weight: weight, design: design))
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
        .hideOverlayHost()
        .preferredColorScheme(.dark)
        .environment(\.colorScheme, .dark)
        .environment(\.hideAccent, HideTheme.color(for: model.core.snapshot?.uiState.accentHex ?? "#B9FF66"))
        .environment(\.hideFontScale, CGFloat((model.core.snapshot?.uiState.fontSize ?? 13) / 13))
        .tint(HideTheme.color(for: model.core.snapshot?.uiState.accentHex ?? "#B9FF66"))
        .sheet(isPresented: $model.showComposer) {
            ChatComposerSheet().hideOverlayHost()
                .environmentObject(model)
        }
        .sheet(isPresented: $model.showSearch) {
            HideSearchSheet().hideOverlayHost()
                .environmentObject(model)
        }
        .sheet(isPresented: $model.showFileSearch) {
            WorkspaceFileSearchSheet().hideOverlayHost()
                .environmentObject(model)
        }
        .sheet(isPresented: $model.showSettings) {
            HideSettingsView(model: model, showsCloseButton: true).hideOverlayHost()
        }
        .sheet(isPresented: $model.showPetDashboard) {
            PetDashboardView()
                .environmentObject(model)
        }
        .sheet(item: $model.worktreeWorkspace) { workspace in
            WorktreeCreationSheet(workspace: workspace)
                .environmentObject(model)
        }
        .sheet(item: $model.branchMigration) { request in
            VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
                Text("Move \(request.branch) out of the main worktree?")
                    .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                Text(request.consequence)
                    .hideFont(size: HideTheme.Typography.body)
                    .foregroundStyle(HideTheme.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                HStack {
                    Spacer()
                    Button("Cancel") { model.branchMigration = nil }
                        .keyboardShortcut(.cancelAction)
                    Button("Move branch", action: model.confirmBranchMigration)
                        .keyboardShortcut(.defaultAction)
                }
            }
            .padding(HideTheme.spacingXL)
            .frame(width: HideTheme.worktreeDialogWidth)
            .background(HideTheme.panel)
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
                    .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                Text(worktree.deletionConsequence)
                    .hideFont(size: HideTheme.Typography.body)
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

private struct WorktreeCreationSheet: View {
    @EnvironmentObject private var model: ShellModel
    let workspace: CoreWorkspaceSnapshot

    private var working: Bool { model.core.snapshot?.taskOperation?.phase == "working" }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                Text("New worktree")
                    .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                Label(workspace.label, systemImage: "folder")
                    .hideFont(size: HideTheme.Typography.body)
                    .foregroundStyle(HideTheme.secondary)
            }
            VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                Text("Branch name")
                    .hideFont(size: HideTheme.Typography.body, weight: .medium)
                    .foregroundStyle(HideTheme.secondary)
                HideSettingsField(
                    placeholder: "feature/your-task",
                    text: $model.worktreeDraft.branch,
                    height: HideTheme.formControlHeight
                )
                .accessibilityLabel("Branch name")
                .accessibilityIdentifier("worktree-branch")
                .disabled(working)
            }
            HideFormPicker(
                "Create from", selection: $model.worktreeDraft.baseBranch,
                selectedLabel: model.worktreeDraft.baseBranch ?? "Select a base branch"
            ) {
                ForEach(model.worktreeBranches, id: \.self) { branch in
                    Text(branch).tag(Optional(branch))
                }
                if model.worktreeDraft.baseBranch == nil {
                    Text("Select a base branch").tag(String?.none)
                }
            }
            .disabled(working)
            HideFormPicker(
                "Start with", selection: $model.worktreeDraft.agent,
                selectedLabel: model.worktreeDraft.agent?.rawValue.capitalized ?? "Terminal only"
            ) {
                Text("Terminal only").tag(AgentProvider?.none)
                ForEach(AgentProvider.allCases, id: \.rawValue) { provider in
                    Text(provider.rawValue.capitalized).tag(Optional(provider))
                }
            }
            .disabled(working)
            if model.worktreeBranches.isEmpty {
                Text("Create the repository's first branch before creating a worktree.")
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.warning)
            }
            if let error = model.worktreeError {
                Text(error)
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.danger)
                    .accessibilityIdentifier("worktree-error")
            }
            HStack {
                Spacer()
                if working {
                    ProgressView(WorktreeSubmissionPresentation.primaryLabel(phase: "working"))
                } else {
                    Button("Cancel", action: model.cancelNewWorktree)
                        .buttonStyle(HideTextButtonStyle(isProminent: false))
                        .keyboardShortcut(.cancelAction)
                    Button(WorktreeSubmissionPresentation.primaryLabel(phase: nil), action: model.submitNewWorktree)
                        .buttonStyle(HideTextButtonStyle(isProminent: true))
                        .keyboardShortcut(.defaultAction)
                        .disabled(!model.worktreeCanSubmit)
                }
            }
        }
        .padding(HideTheme.spacingXL)
        .frame(width: HideTheme.worktreeDialogWidth)
        .background(HideTheme.panel)
        .interactiveDismissDisabled(working)
        .onDisappear {
            if !working { model.cancelNewWorktree() }
        }
    }
}

private struct AgentSwitcherOverlay: View {
    let cycle: AgentSwitcherCycle
    let agents: [SidebarAgent]

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            Text("RECENT AGENTS")
                .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                .foregroundStyle(HideTheme.muted)
            ForEach(cycle.paneIDs, id: \.self) { paneID in
                if let agent = agents.first(where: { $0.paneID == paneID }) {
                    HStack(spacing: HideTheme.spacingMD) {
                        if let mark = AgentMark.image(for: agent.agentKind) {
                            Image(nsImage: mark)
                                .resizable()
                                .aspectRatio(contentMode: .fit)
                                .frame(width: 19, height: 19)
                        }
                        VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                            Text(agent.summary)
                                .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                            Text("\(agent.contextLabel) · \(paneID)")
                                .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                                .foregroundStyle(HideTheme.secondary)
                        }
                        Spacer()
                    }
                    .padding(.horizontal, HideTheme.spacingMD)
                    .frame(height: 44)
                    .background(
                        paneID == cycle.selectedPaneID ? HideTheme.accent.opacity(HideTheme.Opacity.emphasisFill) : Color.clear,
                        in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    )
                }
            }
        }
        .padding(HideTheme.spacingMD)
        .frame(width: 360)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusLarge))
        .overlay {
            RoundedRectangle(cornerRadius: HideTheme.radiusLarge).stroke(HideTheme.divider)
        }
        .accessibilityIdentifier("agent-mru-switcher")
    }
}

private struct TabSwitcherOverlay: View {
    let cycle: TabSwitcherCycle
    let tabs: [ShellTabItem]
    let checkoutLabel: String?

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            Text("RECENT TABS")
                .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                .foregroundStyle(HideTheme.muted)
            ForEach(cycle.tabIDs, id: \.self) { tabID in
                if let tab = tabs.first(where: { $0.id == tabID }) {
                    HStack(spacing: HideTheme.spacingMD) {
                        Image(systemName: icon(for: tab))
                            .hideFont(size: HideTheme.Typography.title, weight: .semibold)
                            .foregroundStyle(HideTheme.secondary)
                            .frame(width: 19, height: 19)
                        VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                            Text(tab.label)
                                .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                                .lineLimit(1)
                            Text(detail(for: tab))
                                .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
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
                    .padding(.horizontal, HideTheme.spacingMD)
                    .frame(height: 44)
                    .background(
                        tabID == cycle.selectedTabID ? HideTheme.accent.opacity(HideTheme.Opacity.emphasisFill) : Color.clear,
                        in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    )
                }
            }
        }
        .padding(HideTheme.spacingMD)
        .frame(width: 360)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusLarge))
        .overlay {
            RoundedRectangle(cornerRadius: HideTheme.radiusLarge).stroke(HideTheme.divider)
        }
        .accessibilityIdentifier("tab-mru-switcher")
    }

    private func icon(for tab: ShellTabItem) -> String {
        switch tab.kind {
        case .herdr: "terminal"
        case .editor(let tab): tab.kind == .diff ? "doc.text.magnifyingglass" : "doc.text"
        }
    }

    private func detail(for tab: ShellTabItem) -> String {
        let kind = switch tab.kind {
        case .herdr: "Terminal"
        case .editor(let tab): tab.kind == .diff ? "Diff" : "File"
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
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.PetDashboard.itemInset) {
                Image(systemName: "pawprint.fill")
                    .foregroundStyle(HideTheme.PetDashboard.accent)
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    Text("Agent dashboard")
                        .hideFont(size: HideTheme.Typography.headline, weight: .bold)
                    Text("Live state from the current Herdr snapshot")
                        .hideFont(size: HideTheme.Typography.caption)
                        .foregroundStyle(HideTheme.PetDashboard.secondary)
                }
                Spacer()
                Text(projection.connection)
                    .hideFont(size: HideTheme.Typography.caption, weight: .semibold, design: .monospaced)
                    .foregroundStyle(projection.connection == "connected" ? HideTheme.success : HideTheme.warning)
                Button("Done", action: { dismiss() })
                    .keyboardShortcut(.cancelAction)
            }
            .padding(HideTheme.PetDashboard.contentInset)

            HStack(spacing: HideTheme.spacingSM) {
                PetCountTile(label: "TOTAL", value: projection.counts.total, color: HideTheme.PetDashboard.primary)
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
                    color: HideTheme.agentWorking
                )
                PetCountTile(
                    label: AgentGroup.seen.title.uppercased(),
                    value: projection.counts.seen,
                    color: HideTheme.PetDashboard.secondary
                )
                PetCountTile(
                    label: "DISCONNECTED",
                    value: projection.counts.disconnected,
                    color: HideTheme.secondary
                )
            }
            .padding(.horizontal, HideTheme.PetDashboard.contentInset)
            .padding(.bottom, HideTheme.PetDashboard.countBottomInset)

            if projection.connection != "connected" {
                HStack(alignment: .top, spacing: HideTheme.spacingSM) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(HideTheme.warning)
                    Text(projection.connectionMessage ?? "Herdr is disconnected. Rows show the last known agents as disconnected.")
                        .hideFont(size: HideTheme.Typography.caption)
                        .foregroundStyle(HideTheme.PetDashboard.secondary)
                }
                .padding(HideTheme.PetDashboard.itemInset)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(HideTheme.warning.opacity(HideTheme.Opacity.subtleFill), in: RoundedRectangle(cornerRadius: HideTheme.PetDashboard.cardRadius))
                .padding(.horizontal, HideTheme.PetDashboard.contentInset)
                .padding(.bottom, HideTheme.PetDashboard.itemInset)
            }

            Divider().overlay(HideTheme.PetDashboard.divider)

            if projection.groups.isEmpty {
                ContentUnavailableView(
                    "No agents",
                    systemImage: "sparkles",
                    description: Text("Start a Claude or Codex agent to see its live status here.")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: HideTheme.spacingMD) {
                        ForEach(projection.groups) { group in
                            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                                HStack {
                                    Text(group.label.uppercased())
                                        .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                                        .tracking(1)
                                        .foregroundStyle(HideTheme.PetDashboard.muted)
                                    Spacer()
                                    Text("\(group.agents.count)")
                                        .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                                        .foregroundStyle(HideTheme.PetDashboard.muted)
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
                    .padding(HideTheme.PetDashboard.contentInset)
                }
            }
        }
        .frame(width: 760, height: 560)
        .background(HideTheme.PetDashboard.panel)
        .preferredColorScheme(.dark)
        .environment(\.hidePetAppearance, true)
        .accessibilityIdentifier("pet-agent-dashboard")
    }
}

private struct PetCountTile: View {
    let label: String
    let value: Int
    let color: Color

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.PetDashboard.countGap) {
            Text(label)
                .hideFont(size: HideTheme.PetDashboard.countLabelSize, weight: .bold)
                .tracking(0.7)
                .foregroundStyle(HideTheme.PetDashboard.muted)
            Text("\(value)")
                .hideFont(size: HideTheme.PetDashboard.countValueSize, weight: .bold, design: .rounded)
                .foregroundStyle(color)
        }
        .padding(.horizontal, HideTheme.PetDashboard.itemInset)
        .frame(maxWidth: .infinity, minHeight: 54, alignment: .leading)
        .background(HideTheme.PetDashboard.elevated, in: RoundedRectangle(cornerRadius: HideTheme.PetDashboard.cardRadius))
    }
}

/// A pet dashboard row: the one agent row on this surface's own card. What the
/// row says is the row's business, so the dashboard adds only the card.
private struct PetDashboardAgentRow: View {
    @Environment(\.hideAccent) private var accent
    let agent: PetDashboardRow
    let action: () -> Void

    private var rowStyle: AgentRowStyle {
        AgentRowStyle(
            titleColor: HideTheme.PetDashboard.primary,
            secondary: HideTheme.PetDashboard.secondary,
            muted: HideTheme.PetDashboard.muted,
            contentSpacing: HideTheme.PetDashboard.rowGap
        )
    }

    var body: some View {
        AgentRow(
            presentation: AgentRowPresentation(row: agent),
            style: rowStyle,
            action: action
        )
        .frame(minHeight: 56)
        .background(
            HideTheme.PetDashboard.elevated.opacity(HideTheme.PetDashboard.cardOpacity),
            in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
        )
        .accessibilityIdentifier("pet-dashboard-agent-\(agent.paneID)")
    }
}

private struct HideSidebar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            HideBrandHeader()
            SidebarContentPicker()
            SidebarCommandBar()

            SidebarList {
                switch model.sidebarContent {
                case .projects:
                    projectsContent
                case .agents:
                    agentsContent
                }
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
        // The one thing the operator does most often is at the top, above
        // everything that is only a place to look.
        NewChatRow()

        // What is waiting, then what finished while the operator was away,
        // come before where things live: they are the only parts that ask for
        // an action. An empty group is not drawn at all.
        ForEach(model.raisedAgentSections) { section in
            HideSectionLabel(title: section.group.title, count: section.agents.count)
            ForEach(section.agents) { agent in
                AgentNavigatorRow(agent: agent, showsWorkspace: true)
            }
        }

        ScratchSection()

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

/// The row that starts a chat. First in the list, because starting one is
/// the most frequent thing done here and every other row is a place rather
/// than an action.
private struct NewChatRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent

    var body: some View {
        Button(action: { model.openComposer() }) {
            HStack(spacing: HideTheme.spacingSM) {
                Image(systemName: "plus.bubble")
                    .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    .foregroundStyle(accent)
                Text("New chat")
                    .hideFont(size: HideTheme.Typography.body, weight: .medium)
                    .foregroundStyle(HideTheme.primary)
                Spacer(minLength: HideTheme.spacingXS)
                HideKeycap(command: .menu(.newChat), emphasized: model.shortcutHintState.revealed && model.shortcutHintState.modifiers == [.command])
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .frame(maxWidth: .infinity, minHeight: HideTheme.IconButton.standardSize.height)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("hide-new-chat")
    }
}

/// The Scratch section: a header that is always drawn, and the tabs under it
/// once the operator opens it.
///
/// The header stays at a count of zero on purpose. Scratch is a permanent
/// place, and a section that disappeared when it emptied would make the space
/// look like something that has to be created.
private struct ScratchSection: View {
    @EnvironmentObject private var model: ShellModel

    private var scratch: CoreScratchSnapshot { model.scratch }

    var body: some View {
        Button(action: model.toggleScratchExpanded) {
            HStack(spacing: HideTheme.spacingSM) {
                Image(systemName: scratch.expanded ? "chevron.down" : "chevron.right")
                    .hideFont(size: HideTheme.Typography.micro, weight: .bold)
                    .foregroundStyle(HideTheme.muted)
                Text(scratch.label.uppercased())
                    .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                Text("\(scratch.tabs.count)")
                    .hideFont(size: HideTheme.Typography.micro, weight: .medium, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.top, HideTheme.spacingMD)
            .padding(.bottom, HideTheme.spacingSM)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(scratch.expanded ? "Collapse Scratch" : "Expand Scratch")
        .accessibilityIdentifier("hide-scratch-header")

        if scratch.expanded {
            if scratch.tabs.isEmpty {
                EmptySidebarRow(
                    systemImage: "tray",
                    title: "Nothing in Scratch",
                    detail: "\(HideCommand.menu(.newChat).displayString(bindings: model.paneShortcuts)) starts a chat that belongs to no project."
                )
            } else {
                ForEach(model.scratchTabsBelowRaisedSections) { tab in
                    ScratchRow(tab: tab)
                }
            }
        }
    }
}

/// One Scratch row: an agent row when the tab holds an agent, a tab row when
/// it does not. The agent row is the sidebar's own component, so a Scratch
/// chat and a project chat read as the same kind of thing.
private struct ScratchRow: View {
    @EnvironmentObject private var model: ShellModel
    let tab: CoreScratchTabSnapshot

    var body: some View {
        if let agent = model.scratchAgent(for: tab) {
            AgentRow(
                presentation: AgentRowPresentation(
                    agent: agent,
                    title: tab.displayName,
                    connected: model.core.snapshot?.status.herdr.state == "connected"
                ),
                style: .shell(density: .compact),
                density: .compact,
                isFocused: model.focusedPaneID == agent.paneID,
                action: { model.selectAgent(agent) }
            )
            .accessibilityIdentifier("hide-scratch-agent-\(agent.paneID)")
        } else {
            Button(action: { model.focusScratchTab(tab) }) {
                HStack(spacing: HideTheme.spacingSM) {
                    Image(systemName: "terminal")
                        .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                        .foregroundStyle(HideTheme.muted)
                    Text(tab.displayName)
                        .hideFont(size: HideTheme.Typography.body)
                        .foregroundStyle(HideTheme.primary)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, HideTheme.spacingMD + HideTheme.spacingSM)
                .frame(maxWidth: .infinity, minHeight: 28)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("hide-scratch-tab-\(tab.id)")
        }
    }
}

private struct SidebarContentPicker: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: HideTheme.spacingXXS) {
            ForEach(SidebarContent.allCases) { content in
                Button {
                    model.showSidebarContent(content)
                } label: {
                    HStack(spacing: HideTheme.spacingSM) {
                        Image(systemName: content.systemImage)
                            .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                        Text(content.title)
                            .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
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
                .hideTooltip("Show \(content.title)", command: .menu(.toggleSidebarView))
                .accessibilityIdentifier("hide-sidebar-view-\(content.rawValue)")
            }
        }
        .padding(HideTheme.spacingXS)
        .background(HideTheme.panel, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
        .overlay {
            RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.bottom, HideTheme.spacingSM)
        .accessibilityIdentifier("hide-sidebar-view-switcher")
    }
}

private struct SidebarCommandBar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Button(action: model.openSearch) {
                HStack(spacing: HideTheme.spacingSM) {
                    Image(systemName: "magnifyingglass")
                        .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    Text("Search")
                        .hideFont(size: HideTheme.Typography.body, weight: .medium)
                    Spacer(minLength: 4)
                    HideKeycap(command: .menu(.search), emphasized: model.shortcutHintState.revealed && model.shortcutHintState.modifiers == [.command])
                }
                .foregroundStyle(HideTheme.secondary)
                .padding(.horizontal, HideTheme.spacingMD)
                .frame(maxWidth: .infinity, minHeight: HideTheme.IconButton.standardSize.height)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Search projects and agents")
            .hideTooltip(ShellMenuCommand.search.title, command: .menu(.search), inline: true)

            HideIconButton(
                systemImage: "folder.badge.plus",
                help: "New project",
                accessibilityLabel: "New project",
                command: .menu(.newWorkspace),
                action: model.openNewWorkspace
            )
            HideIconButton(
                systemImage: "plus",
                help: "New chat",
                accessibilityLabel: "New chat",
                command: .menu(.newChat),
                action: { model.openComposer() }
            )
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.bottom, HideTheme.spacingMD)
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
        HStack(spacing: HideTheme.spacingXS) {
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
                HStack(spacing: HideTheme.spacingSM) {
                    Circle()
                        .fill(selectedDevice.map(deviceStatusColor) ?? HideTheme.muted)
                        .frame(width: 6, height: 6)
                    Text(selectedDevice?.label ?? "No device")
                        .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                        .lineLimit(1)
                    if let agentCount = selectedDevice?.agentCount, agentCount > 0 {
                        Text("\(agentCount)")
                            .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                            .foregroundStyle(HideTheme.muted)
                    }
                    Image(systemName: "chevron.up.chevron.down")
                        .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                        .foregroundStyle(HideTheme.muted)
                }
                .foregroundStyle(HideTheme.secondary)
                .padding(.horizontal, HideTheme.spacingSM)
                .frame(height: 30)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .hideTooltip("Choose device")

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
                                    .hideFont(size: HideTheme.Typography.micro, weight: .semibold, design: .monospaced)
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
            .hideTooltip("Weekly provider usage")
            .accessibilityLabel("Weekly provider usage")
            .popover(isPresented: $showingUsage, arrowEdge: .bottom) {
                HideUsagePopover(usages: usages)
            }

            Button {
                model.showSettings = true
            } label: {
                Image(systemName: "gearshape")
                    .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                    .frame(width: 30, height: 30)
            }
            .buttonStyle(.plain)
            .hideTooltip("Settings")
            .accessibilityLabel("Settings")
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.vertical, HideTheme.spacingSM)
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
                    .hideFont(size: HideTheme.Typography.micro, weight: .bold, design: .rounded)
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
        VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            HStack(spacing: HideTheme.spacingSM) {
                Text("Weekly Usage")
                    .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                Spacer()
                Text("7 days")
                    .hideFont(size: HideTheme.Typography.micro, weight: .semibold, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }

            if usages.isEmpty {
                Text("Provider usage is not available yet.")
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.secondary)
            } else {
                ForEach(usages) { usage in
                    HideWeeklyUsageRow(usage: usage)
                }
            }
        }
        .padding(HideTheme.spacingLG)
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
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            HStack(spacing: HideTheme.spacingSM) {
                HideProviderMark(usage: usage, isMuted: false)

                Text(usage.label)
                    .hideFont(size: HideTheme.Typography.body, weight: .medium)
                    .foregroundStyle(HideTheme.secondary)
                    .lineLimit(1)
                Spacer(minLength: 8)
                Text(valueLabel)
                    .hideFont(size: HideTheme.Typography.caption, weight: .semibold, design: .monospaced)
                    .foregroundStyle(usage.state == "available" ? usageColor : HideTheme.muted)
            }

            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                        .fill(HideTheme.divider)
                    RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                        .fill(usageColor)
                        .frame(width: geometry.size.width * clampedProgress)
                }
            }
            .frame(height: 3)
            .accessibilityHidden(true)
        }
        .hideTooltip(helpText)
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
                .hideFont(size: HideTheme.Typography.headline, weight: .bold, design: .rounded)
                .tracking(-0.6)
                .foregroundStyle(HideTheme.primary)
            Circle()
                .fill(model.herdrIsConnected ? HideTheme.success : HideTheme.warning)
                .frame(width: 6, height: 6)
            Spacer()
            Text(model.isRemoteContext ? model.remote.targetLabel : (model.core.runtimeSelection?.version ?? "offline"))
                .hideFont(size: HideTheme.Typography.micro, weight: .medium, design: .monospaced)
                .foregroundStyle(HideTheme.muted)
                .lineLimit(1)
                .truncationMode(.middle)
                .hideTooltip(model.isRemoteContext ? model.remote.targetLabel : (model.core.runtimeSelection?.version ?? "offline"))
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
            .hideTooltip("Hide left sidebar", command: .menu(.toggleLeftSidebar))
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
        HStack(spacing: HideTheme.spacingSM) {
            Text(title)
                .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                .foregroundStyle(HideTheme.secondary)
            if let count {
                Text("\(count)")
                    .hideFont(size: HideTheme.Typography.micro, weight: .medium, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            Spacer()
        }
        .padding(.horizontal, HideTheme.spacingLG)
        .padding(.top, HideTheme.spacingMD)
        .padding(.bottom, HideTheme.spacingSM)
    }
}

private struct EmptySidebarRow: View {
    let systemImage: String
    let title: String
    let detail: String

    var body: some View {
        HStack(alignment: .top, spacing: HideTheme.spacingSM) {
            Image(systemName: systemImage)
                .foregroundStyle(HideTheme.muted)
                .frame(width: 16)
            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                Text(title)
                    .hideFont(size: HideTheme.Typography.subhead, weight: .medium)
                    .foregroundStyle(HideTheme.secondary)
                Text(detail)
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.horizontal, HideTheme.spacingLG)
        .padding(.vertical, HideTheme.spacingMD)
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

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
            HStack(spacing: HideTheme.spacingNone) {
                Button {
                    model.toggleWorkspace(workspace)
                } label: {
                    HStack(spacing: HideTheme.spacingSM) {
                        Image(systemName: workspace.expanded ? "chevron.down" : "chevron.right")
                            .hideFont(size: HideTheme.Typography.micro, weight: .bold)
                            .foregroundStyle(HideTheme.muted)
                            .frame(width: 12, height: 20)
                        Image(systemName: workspace.isGit ? "folder.badge.gearshape" : "folder")
                            .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                            .foregroundStyle(
                                workspace.temporary
                                    ? HideTheme.warning
                                    : (isFocusedWorkspace ? accent : HideTheme.secondary)
                            )
                            .frame(width: 16)
                        Text(workspace.label)
                            .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                            .foregroundStyle(isFocusedWorkspace ? HideTheme.primary : HideTheme.secondary)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        Text(presentation.activityLabel)
                            .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
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
                    Button(WorktreeMenuPolicy.newWorktree) { model.requestNewWorktree(workspace) }
                        .disabled(!workspace.isGit || workspace.remoteTargetID != nil)
                    Divider()
                    Button(WorktreeMenuPolicy.removeRegistration, role: .destructive) {
                        model.requestRemoveWorkspace(workspace)
                    }
                } label: {
                    Image(systemName: "ellipsis")
                        .hideFont(size: HideTheme.Typography.body, weight: .bold)
                        .foregroundStyle(HideTheme.muted)
                        .frame(width: 22, height: 22)
                        .contentShape(Rectangle())
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .frame(width: 24, height: 28)
            }
            .padding(.leading, HideTheme.spacingMD)
            .padding(.trailing, HideTheme.spacingSM)

            if workspace.expanded {
                ForEach(workspace.checkouts) { checkout in
                    checkoutGroup(checkout)
                }
            }
        }
        .padding(.bottom, HideTheme.spacingXS)
    }

    private func checkoutGroup(_ checkout: CoreCheckoutSnapshot) -> some View {
        let isFocused = model.focusedCheckout?.id == checkout.id
        let visibleAgents = SidebarGrouping.tree(model.agents, checkoutID: checkout.id,
            ownedPaneIDs: Set(checkout.tabs.flatMap(\.panes).map(\.id)))
        let checkoutPresentation = SidebarCheckoutPresentation(
            workspace: workspace,
            checkout: checkout,
            agents: model.agents,
            connected: model.agentsConnected
        )

        return VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            CheckoutNavigatorRow(
                workspace: workspace,
                checkout: checkout,
                presentation: checkoutPresentation,
                isFocused: isFocused,
                hasAgents: !visibleAgents.isEmpty
            )
            if model.isCheckoutExpanded(checkout) {
                ForEach(visibleAgents) { agent in
                    AgentNavigatorRow(agent: agent, showsWorkspace: false)
                }
            }
        }
        .background(
            isFocused ? HideTheme.elevated.opacity(HideTheme.Opacity.secondary) : .clear,
            in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
        )
        .overlay {
            if isFocused {
                RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
        }
        .padding(.horizontal, HideTheme.spacingSM)
    }
}

/// A compact read-only summary; the surrounding Workspace row owns disclosure.
private struct WorkspaceAgentSummary: View {
    let presentation: SidebarCheckoutPresentation

    var body: some View {
        HStack(spacing: HideTheme.spacingXS) {
            if let status = presentation.status {
                AgentStatusMark(symbol: status.symbol, color: status.color)
            }
            AgentBadge(
                agentKind: presentation.representativeAgentKind ?? "terminal",
                stateColor: presentation.status?.color ?? HideTheme.secondary,
                size: HideTheme.lineageChevronWidth
            )
            if presentation.agentCount > 1 {
                Text("+\(presentation.agentCount - 1)")
                    .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
            }
        }
        .padding(.horizontal, HideTheme.spacingXS)
        .frame(height: HideTheme.IconButton.toolbarSize.height)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
        .fixedSize()
        .accessibilityHidden(true)
    }
}

private struct CheckoutNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    let workspace: CoreWorkspaceSnapshot
    let checkout: CoreCheckoutSnapshot
    let presentation: SidebarCheckoutPresentation
    let isFocused: Bool
    let hasAgents: Bool

    var body: some View {
        Button {
            if hasAgents {
                model.toggleCheckoutExpansion(checkout)
            } else {
                model.selectCheckout(checkout)
            }
        } label: {
            HStack(spacing: HideTheme.spacingSM) {
                Color.clear.frame(width: HideTheme.agentMarkWidth)
                Image(systemName: workspace.isGit ? "arrow.triangle.branch" : "folder")
                    .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                    .frame(width: HideTheme.checkoutIconWidth)
                Text(checkout.label)
                    .hideFont(size: HideTheme.Typography.subhead, weight: isFocused ? .semibold : .medium)
                    .foregroundStyle(HideTheme.primary)
                    .lineLimit(1)
                if presentation.isDetached {
                    HideBadge(label: "detached", color: HideTheme.secondary)
                        .hideTooltip(presentation.detailTooltip)
                }
                if !checkout.exists {
                    HideBadge(label: "missing", color: HideTheme.danger)
                } else if checkout.temporary {
                    HideBadge(label: "temporary", color: HideTheme.warning)
                }
                if presentation.isPrimary {
                    HideBadge(label: "primary", color: HideTheme.secondary)
                    if case .warning(let branch, _) = MainWorktreePresentation.state(
                        branch: checkout.branch, base: model.baseBranch(for: workspace)
                    ) {
                        Button {
                            model.requestBranchMigration(workspace: workspace, checkout: checkout)
                        } label: {
                            Image(systemName: "exclamationmark.triangle")
                                .foregroundStyle(HideTheme.warning)
                        }
                        .buttonStyle(.plain)
                        .hideTooltip("Move \(branch) to a worktree")
                        .accessibilityLabel("Move \(branch) to a worktree")
                    }
                }
                if let pullRequest = checkout.pullRequest {
                    HideBadge(
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
                        .hideTooltip("\(checkout.changedFileCount) uncommitted changes")
                }
                Spacer(minLength: 0)
                if presentation.agentCount > 0 {
                    WorkspaceAgentSummary(presentation: presentation)
                }
                Image(systemName: model.isCheckoutExpanded(checkout) ? "chevron.down" : "chevron.right")
                    .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                    .frame(width: HideTheme.lineageChevronWidth)
                    .opacity(hasAgents ? 1 : 0)
                    .accessibilityHidden(true)
            }
            .padding(.horizontal, HideTheme.spacingSM)
            .frame(minHeight: HideTheme.checkoutRowHeight)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .hideTooltip(presentation.detailTooltip)
        .accessibilityIdentifier("hide-checkout-\(checkout.id)")
        // The row is deliberately almost wordless, so everything the colours,
        // dots, and badges carry is said here in words (G1, design 7).
        .accessibilityLabel(
            CheckoutCardPresentation.rowAccessibilityLabel(
                repoName: workspace.repoName,
                checkout: checkout,
                agentCount: presentation.agentCount
            ) + (presentation.status.map { ". \($0.label)" } ?? "")
        )
        .accessibilityValue(hasAgents
            ? (model.isCheckoutExpanded(checkout) ? "Expanded" : "Collapsed")
            : (isFocused ? "Selected" : "Not selected"))
        .accessibilityHint(hasAgents ? "Show or hide agents in this workspace" : "Open this workspace")
        .contextMenu {
            Button(WorktreeMenuPolicy.newWorktree, systemImage: "plus") { model.requestNewWorktree(workspace) }
            if checkout.isWorktree {
                Button(WorktreeMenuPolicy.startAgentHere, systemImage: "terminal") { model.openComposer(checkoutID: checkout.id) }
            }
            if let branch = checkout.branch {
                Button(WorktreeMenuPolicy.setBaseBranch, systemImage: "arrow.triangle.branch") { model.setBaseBranch(checkout, in: workspace) }
                    .disabled(branch == model.baseBranch(for: workspace))
            }
            Divider()
            Button(WorktreeMenuPolicy.copyPath, systemImage: "doc.on.doc") { model.copyCheckoutPath(checkout) }
            Menu(WorktreeMenuPolicy.openIn, systemImage: "arrow.up.forward.app") {
                Button("Finder") { model.revealCheckout(checkout) }
                Button("Default editor") { model.openCheckoutInDefaultEditor(checkout) }
            }
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

/// A sidebar agent row: the one agent row plus the sidebar's focus state and
/// its direct-select shortcut hint.
private struct AgentNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    let agent: SidebarAgent
    /// Under a checkout the project name is the heading above the row, so
    /// repeating it wastes the line the summary needs.
    let showsWorkspace: Bool

    private var density: AgentRowDensity { showsWorkspace ? .prominent : .compact }
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
            HStack(spacing: HideTheme.spacingNone) {
                if !showsWorkspace {
                    Button {
                        model.core.dispatch(kind: "agent_tree_toggle", payload: ["pane_id": agent.paneID])
                    } label: {
                        Image(systemName: agent.lineageCollapsed ? "chevron.right" : "chevron.down")
                            .foregroundStyle(HideTheme.muted)
                    }
                    .buttonStyle(.plain)
                    .frame(width: HideTheme.lineageChevronWidth)
                    .opacity(agent.lineageChildPaneIDs.isEmpty ? 0 : 1)
                    .disabled(agent.lineageChildPaneIDs.isEmpty)
                    .accessibilityHidden(agent.lineageChildPaneIDs.isEmpty)
                    .hideTooltip(agent.lineageCollapsed ? "Expand descendants" : "Collapse descendants")
                }
                AgentRow(
                    presentation: AgentRowPresentation(
                        agent: agent,
                        density: density,
                        connected: model.agentsConnected
                    ),
                    style: .shell(density: density),
                    density: density,
                    isFocused: model.focusedPaneID == agent.paneID,
                    shortcutNumber: model.agentShortcutNumber(paneID: agent.paneID),
                    shortcutVisible: model.shortcutHintState.revealed && model.shortcutHintState.modifiers == [.control],
                    action: { model.selectAgent(agent) }
                )
            }
            if !showsWorkspace, let badge = agent.lineageWorktreeBadge {
                HideBadge(label: badge, color: HideTheme.secondary)
            }
            if let hint = showsWorkspace ? agent.raisedHint : agent.lineageHint {
                Text(hint).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.muted)
            }
        }
        .overlay(alignment: .leading) {
            if !showsWorkspace && agent.lineageDepth > 0 {
                HStack(spacing: HideTheme.spacingNone) {
                    Rectangle().frame(width: HideTheme.Layout.hairlineWidth)
                    Rectangle().frame(width: HideTheme.spacingSM, height: HideTheme.Layout.hairlineWidth)
                }
                .foregroundStyle(HideTheme.divider)
                .frame(width: HideTheme.lineageChevronWidth, alignment: .leading)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
            }
        }
        .padding(.leading, showsWorkspace ? HideTheme.spacingNone : HideTheme.lineageInset(depth: agent.lineageDepth))

        .animation(.easeOut(duration: HideTooltipState.fadeDuration(reduceMotion: reduceMotion)), value: (model.shortcutHintState.revealed && model.shortcutHintState.modifiers == [.control]))
        .accessibilityIdentifier("hide-agent-\(agent.id)")
        .hideTooltip(agent.summary, command: model.agentShortcutNumber(paneID: agent.paneID).map(HideCommand.agent), inline: true)
    }
}

private struct HideMainView: View {
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
                                            Text(tab.label)
                                                .hideFont(size: HideTheme.Typography.body, weight: tab.active ? .semibold : .medium)
                                                .lineLimit(1)
                                                .frame(maxWidth: HideTheme.tabTitleMaxWidth, alignment: .leading)
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
                                    .buttonStyle(.plain)
                                    .hideTooltip([tab.contextLabel ?? tab.label, tab.focusedAgent.map { AgentStatusPresentation(agent: $0, connected: model.agentsConnected).label }].compactMap { $0 }.joined(separator: " · "), command: model.tabShortcutNumber(tabID: tab.id).map(HideCommand.tab), inline: true)

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

struct HideToolbarButtonStyle: ButtonStyle {
    let isProminent: Bool
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .hideFont(size: HideTheme.Typography.body, weight: .semibold)
            .foregroundStyle(isProminent ? HideTheme.background : HideTheme.primary)
            .padding(.horizontal, isProminent ? HideTheme.spacingMD : HideTheme.spacingSM)
            .padding(.vertical, HideTheme.spacingSM)
            .background(isProminent ? HideTheme.accent : HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .opacity(!isEnabled ? HideTheme.Opacity.disabled : (configuration.isPressed ? HideTheme.Opacity.secondary : 1))
    }
}

private struct HideTerminalSurface: View {
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
                        .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(HideTheme.spacingXXXL)
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
                        .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                        .accessibilityIdentifier("hide-empty-state-new-workspace")
                } else {
                    switch model.checkoutStartState {
                    case .starting:
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
                    case .started:
                        Text("Terminal is starting")
                            .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text("Waiting for Herdr to attach the new pane to this checkout.")
                            .hideFont(size: HideTheme.Typography.subhead)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 360)
                    case let .failed(message):
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
                            .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                    case .idle:
                        // This state used to promise a terminal Hide never
                        // started: nothing calls `startTerminal` from here.
                        // The empty state now carries the control that starts
                        // one, and says only what pressing it does.
                        Text("No terminal open")
                            .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text("This checkout has no terminal pane. Start one to fill the workspace at its path.")
                            .hideFont(size: HideTheme.Typography.subhead)
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
            .padding(HideTheme.spacingXXXL)
        }
    }
}

private struct HideStatusBar: View {
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

/// The chat composer: three chips on one line, a message, and Send.
///
/// It replaces the form this shell used to open. That form asked four
/// questions before the
/// operator could type anything and refused to start without a checkout; this
/// asks one - what do you want - and answers the other three with defaults
/// already filled in.
private struct ChatComposerSheet: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.hideAccent) private var accent
    @State private var message = ""
    @State private var provider: AgentProvider = .claude
    @State private var bypassWarnings = false
    @FocusState private var messageFocused: Bool

    private var agentIsInstalled: Bool {
        AgentCLIAvailability.isUsable(provider.rawValue)
    }

    private var canSend: Bool {
        ChatComposerPolicy.canSend(
            message: message,
            agentIsInstalled: agentIsInstalled,
            isSubmitting: model.composerSubmitting
        )
    }

    private var whereLabel: String {
        guard let checkout = model.composerCheckout else { return model.scratch.label }
        return checkout.label
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            chips
            messageField
            footer
        }
        .padding(HideTheme.spacingLG)
        .frame(width: HideTheme.composerSheetSize.width, height: HideTheme.composerSheetSize.height)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .accessibilityIdentifier("hide-chat-composer")
        .onAppear {
            provider = AgentProvider(rawValue: model.core.snapshot?.uiState.lastAgentKind ?? "")
                ?? .claude
            bypassWarnings = model.core.snapshot?.uiState.lastAgentBypass ?? false
            messageFocused = true
        }
        .onExitCommand {
            guard !model.composerSubmitting else { return }
            dismiss()
        }
    }

    /// Where, Run on, Agent - one line, each already answered.
    private var chips: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Menu {
                Button(model.scratch.label) { model.composerCheckoutID = nil }
                ForEach(model.composerCheckouts, id: \.checkout.id) { item in
                    Button("\(item.workspace.repoName) / \(item.checkout.label)") {
                        model.composerCheckoutID = item.checkout.id
                    }
                }
            } label: {
                ComposerChipLabel(icon: "tray", title: whereLabel)
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .accessibilityIdentifier("hide-composer-where")

            Menu {
                ForEach(model.devices) { device in
                    Button(device.label) { model.composerDeviceID = device.id }
                }
            } label: {
                ComposerChipLabel(
                    icon: "desktopcomputer",
                    title: model.devices.first(where: { $0.id == model.composerDeviceID })?.label
                        ?? "This Mac"
                )
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .accessibilityIdentifier("hide-composer-device")

            Menu {
                ForEach(AgentProvider.allCases, id: \.rawValue) { candidate in
                    Button(candidate.rawValue.capitalized) { provider = candidate }
                }
                Divider()
                Toggle("Pass the CLI bypass flag", isOn: $bypassWarnings)
            } label: {
                ComposerChipLabel(
                    icon: "sparkles",
                    title: provider.rawValue.capitalized,
                    markKind: provider.rawValue
                )
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .accessibilityIdentifier("hide-composer-agent")

            if bypassWarnings {
                Label(
                    "Bypass flag on",
                    systemImage: "exclamationmark.triangle.fill"
                )
                .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                .foregroundStyle(HideTheme.warning)
                .accessibilityIdentifier("hide-composer-bypass-warning")
            }
            Spacer(minLength: 0)
        }
    }

    private var messageField: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            TextEditor(text: $message)
                .focused($messageFocused)
                .hideFont(size: HideTheme.Typography.title)
                .foregroundStyle(HideTheme.primary)
                .scrollContentBackground(.hidden)
                .padding(HideTheme.spacingSM)
                .background(
                    HideTheme.elevated,
                    in: RoundedRectangle(cornerRadius: HideTheme.radiusLarge)
                )
                .overlay {
                    RoundedRectangle(cornerRadius: HideTheme.radiusLarge)
                        .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                }
                .overlay(alignment: .topLeading) {
                    if message.isEmpty {
                        Text("Ask anything")
                            .hideFont(size: HideTheme.Typography.title)
                            .foregroundStyle(HideTheme.muted)
                            .padding(.horizontal, HideTheme.spacingMD)
                            .padding(.vertical, HideTheme.spacingLG)
                            .allowsHitTesting(false)
                    }
                }
                .disabled(model.composerSubmitting)
                .accessibilityIdentifier("hide-composer-message")

            if !agentIsInstalled {
                VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                    Label(
                        "\(provider.rawValue) is not on the login-shell PATH.",
                        systemImage: "exclamationmark.triangle"
                    )
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.warning)
                    Link("Install \(provider.rawValue)", destination: provider == .claude
                        ? URL(string: "https://docs.anthropic.com/en/docs/claude-code/overview")!
                        : URL(string: "https://developers.openai.com/codex/")!)
                        .hideFont(size: HideTheme.Typography.caption)
                }
                .accessibilityIdentifier("hide-composer-agent-missing")
            }
        }
    }

    private var footer: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Spacer(minLength: 0)
            Button(action: send) {
                HStack(spacing: HideTheme.spacingXS + 2) {
                    if model.composerSubmitting {
                        ProgressView()
                            .controlSize(.small)
                        Text("Starting")
                            .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    } else {
                        Text("Send")
                            .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                        HideKeycap(command: .label(PaneShortcut(key: "↩", modifiers: [.command]).displayString), emphasized: false)
                            .opacity(HideTheme.Opacity.secondary)
                    }
                }
                .foregroundStyle(canSend ? HideTheme.background : HideTheme.muted)
                .padding(.horizontal, HideTheme.spacingMD)
                .frame(height: 28)
                .background(
                    canSend ? accent : HideTheme.elevated,
                    in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                )
            }
            .buttonStyle(.plain)
            .disabled(!canSend)
            .keyboardShortcut(.return, modifiers: .command)
            .accessibilityIdentifier("hide-composer-send")
        }
    }

    private func send() {
        guard canSend else { return }
        model.sendComposerMessage(
            provider: provider,
            message: message,
            bypassWarnings: bypassWarnings
        )
    }
}

/// One composer chip. The three read as one control strip, so they are drawn
/// once here rather than three times with drifting padding.
private struct ComposerChipLabel: View {
    /// How big the agent mark is drawn. It is applied to the image rather than
    /// to a frame around it, because this label is a `Menu`'s and AppKit sizes
    /// it from the NSImage.
    private static let markSide: CGFloat = 12

    let icon: String
    let title: String
    /// The agent kind whose mark replaces `icon`, on the chip that names one.
    var markKind: String?

    var body: some View {
        HStack(spacing: HideTheme.spacingXS) {
            if let markKind, let mark = AgentMark.image(for: markKind, side: Self.markSide) {
                Image(nsImage: mark)
                    .interpolation(.high)
            } else {
                Image(systemName: icon)
                    .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
            }
            Text(title)
                .hideFont(size: HideTheme.Typography.body, weight: .medium)
                .lineLimit(1)
            Image(systemName: "chevron.down")
                .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                .foregroundStyle(HideTheme.muted)
        }
        .foregroundStyle(HideTheme.secondary)
        .padding(.horizontal, HideTheme.spacingMD)
        .frame(height: 26)
        .background(HideTheme.elevated, in: Capsule())
        .overlay {
            Capsule().stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
        }
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
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingSM) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(accent)
                TextField("Search agents and workspaces", text: $query)
                    .textFieldStyle(.plain)
                    .hideFont(size: HideTheme.Typography.headline)
                    .onSubmit { if let first = entries.first { route(first) } }
                Text("ESC")
                    .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            .padding(HideTheme.spacingLG)
            .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusLarge))
            .padding(HideTheme.spacingLG)
            ScrollView {
                LazyVStack(spacing: HideTheme.spacingXXS) {
                    ForEach(agentGroups) { group in
                        Text("\(group.workspace) > AGENTS")
                            .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                            .foregroundStyle(HideTheme.muted)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, HideTheme.spacingMD)
                            .padding(.top, HideTheme.spacingSM)
                        ForEach(group.entries) { entry in
                            searchButton(entry)
                        }
                    }
                    if !checkoutEntries.isEmpty {
                        Text("WORKSPACES > CHECKOUTS")
                            .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                            .foregroundStyle(HideTheme.muted)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, HideTheme.spacingMD)
                            .padding(.top, HideTheme.spacingSM)
                        ForEach(checkoutEntries) { entry in
                            searchButton(entry)
                        }
                    }
                    if entries.isEmpty {
                        Text("No matching agents or workspaces")
                            .hideFont(size: HideTheme.Typography.subhead)
                            .foregroundStyle(HideTheme.secondary)
                            .padding(HideTheme.spacingXXL)
                    }
                }
                .padding(.horizontal, HideTheme.spacingLG)
            }
        }
        .frame(width: HideTheme.searchSheetSize.width, height: HideTheme.searchSheetSize.height)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
    }

    private func searchButton(_ entry: HideSearchEntry) -> some View {
        Button { route(entry) } label: {
            HStack(spacing: HideTheme.spacingMD) {
                Image(systemName: entry.kind.systemImage)
                    .foregroundStyle(accent)
                    .frame(width: 18)
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    Text(entry.title)
                        .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Text(entry.subtitle)
                        .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                        .foregroundStyle(HideTheme.secondary)
                        .lineLimit(1)
                }
                Spacer()
                Text("↵").foregroundStyle(HideTheme.muted)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)
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
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            Text(title)
                .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                .foregroundStyle(HideTheme.primary)
            Text(subtitle)
                .hideFont(size: HideTheme.Typography.body)
                .foregroundStyle(HideTheme.secondary)
        }
        .padding(.horizontal, HideTheme.spacingXL)
        .padding(.top, HideTheme.spacingXL)
        .padding(.bottom, HideTheme.spacingSM)
    }
}
