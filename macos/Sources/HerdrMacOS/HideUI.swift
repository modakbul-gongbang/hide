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
            RecentNavigationOverlay(model: model, presentation: model.recentNavigation)
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
            HideSettingsView(
                model: model,
                showsCloseButton: true,
                initialTab: model.settingsInitialTab
            )
            .hideOverlayHost()
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
                        .buttonStyle(HideTextButtonStyle())
                        .keyboardShortcut(.cancelAction)
                    Button("Move branch", action: model.confirmBranchMigration)
                        .buttonStyle(HideTextButtonStyle(appearance: .prominent))
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
        // The same alert presentation as the workspace removal above, on the
        // current API because the older `Alert` value makes Cancel the
        // Return key's button whenever the other one is destructive. Here
        // Return is Move to Trash and Esc is Cancel (D-03): the chord that
        // opened the modal is a keyboard flow, and its answer stays on the
        // keyboard. Dismissing by any route clears the prompt through the
        // binding and sends nothing.
        .alert(
            model.explorerTrashPrompt?.title ?? "",
            isPresented: Binding(
                get: { model.explorerTrashPrompt != nil },
                set: { if !$0 { model.explorerTrashPrompt = nil } }
            ),
            presenting: model.explorerTrashPrompt
        ) { _ in
            Button(WorkspaceOutlineTrashPrompt.confirmTitle, role: .destructive, action: model.confirmExplorerTrash)
                .keyboardShortcut(.defaultAction)
            Button("Cancel", role: .cancel) { model.explorerTrashPrompt = nil }
        } message: { prompt in
            Text(prompt.message)
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
                        .toggleStyle(HideCheckboxStyle())
                }
                HStack {
                    Spacer()
                    Button("Cancel") { model.worktreeToDelete = nil }
                        .buttonStyle(HideTextButtonStyle())
                        .keyboardShortcut(.cancelAction)
                    Button(worktree.deletionGate.buttonLabel, role: .destructive, action: model.confirmDeleteWorktree)
                        .buttonStyle(HideTextButtonStyle())
                }
            }
            .padding(HideTheme.spacingXL)
            .frame(width: HideTheme.worktreeDialogWidth)
            .background(HideTheme.panel)
        }
        .alert(
            model.herdrProtocolMismatch?.title ?? "Hide and Herdr aren’t compatible",
            isPresented: Binding(
                get: { model.herdrProtocolMismatch != nil },
                set: { if !$0 { model.dismissHerdrProtocolMismatch() } }
            ),
            presenting: model.herdrProtocolMismatch
        ) { details in
            if let actionLabel = details.primaryActionLabel {
                Button(actionLabel) { model.openHerdrProtocolRecovery(details) }
            }
            Button("Copy Diagnostics") { model.copyHerdrProtocolDiagnostics(details) }
            Button("OK", role: .cancel, action: model.dismissHerdrProtocolMismatch)
        } message: { details in
            Text(details.message)
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
        // A close with a consequence - a browser pane, whose Chromium tab goes
        // with it, or a working agent - waits here for the operator's answer.
        // The model holds the pending target, so the header X and ⌘W share
        // one prompt. Return closes and Esc cancels, as in the trash prompt
        // above; dismissing by any other route cancels through the binding.
        .alert(
            model.consequenceNotice?.title ?? "",
            isPresented: Binding(
                get: { model.consequenceNotice != nil },
                set: { if !$0, model.consequenceNotice != nil { model.cancelConsequencePreview() } }
            ),
            presenting: model.consequenceNotice
        ) { _ in
            Button("Close", role: .destructive, action: model.confirmConsequencePreview)
                .keyboardShortcut(.defaultAction)
            Button("Cancel", role: .cancel, action: model.cancelConsequencePreview)
        } message: { notice in
            Text(notice.message)
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
                        .buttonStyle(HideTextButtonStyle())
                        .keyboardShortcut(.cancelAction)
                    Button(WorktreeSubmissionPresentation.primaryLabel(phase: nil), action: model.submitNewWorktree)
                        .buttonStyle(HideTextButtonStyle(appearance: .prominent))
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

private struct RecentNavigationOverlay: View {
    let model: ShellModel
    @ObservedObject var presentation: RecentNavigationPresentation

    var body: some View {
        if let cycle = presentation.projectCycle {
            RecentSwitcherOverlay(
                title: "RECENT PROJECTS", command: .recentProject,
                rows: cycle.visibleIDs.compactMap { id in
                    guard let project = model.recentProjects[id] else { return nil }
                    return RecentSwitcherRow(id: id, title: project.workspace.label,
                        detail: model.recentProjectDetail(id), symbol: "folder")
                }, selectedID: cycle.selectedProjectID,
                identifier: "project-mru-switcher"
            )
        } else if let cycle = presentation.tabCycle {
            RecentSwitcherOverlay(
                title: "RECENT PANELS", command: .recentTab,
                rows: cycle.visibleIDs.compactMap { id in
                    guard let surface = model.recentSurfaces[id] else { return nil }
                    return RecentSwitcherRow(id: id, title: surface.item.label,
                        detail: surface.contextLabel, symbol: surface.symbol, dirty: surface.item.dirty,
                        agent: surface.item.focusedAgent)
                }, selectedID: cycle.selectedTabID,
                identifier: "tab-mru-switcher"
            )
        }
    }
}

private struct RecentSwitcherRow: Identifiable {
    let id: String
    let title: String
    let detail: String
    let symbol: String
    var dirty = false
    var agent: SidebarAgent? = nil
}

/// Both navigation levels share the existing panel, typography and keycaps.
/// The model exposes at most nine rows around the highlight, even in a large session.
private struct RecentSwitcherOverlay: View {
    let title: String
    let command: ShellMenuCommand
    let rows: [RecentSwitcherRow]
    let selectedID: String
    let identifier: String

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            HStack {
                Text(title)
                    .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                    .foregroundStyle(HideTheme.muted)
                Spacer()
                HideKeycap(command: .menu(command))
            }
            ForEach(rows) { row in
                HStack(spacing: HideTheme.spacingMD) {
                    Group {
                        if let agent = row.agent {
                            AgentBadge(agentKind: agent.agentKind,
                                stateColor: HideTheme.secondary,
                                size: HideTheme.checkoutIconWidth)
                        } else {
                            Image(systemName: row.symbol)
                                .hideFont(size: HideTheme.Typography.title, weight: .semibold)
                                .foregroundStyle(HideTheme.secondary)
                        }
                    }
                    .frame(width: HideTheme.checkoutIconWidth)
                    VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                        Text(row.title)
                            .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Text(row.detail)
                            .hideFont(size: HideTheme.Typography.caption)
                            .foregroundStyle(HideTheme.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    Spacer()
                    if row.dirty {
                        Image(systemName: "circle.fill")
                            .hideFont(size: HideTheme.Typography.caption)
                            .foregroundStyle(HideTheme.secondary)
                            .accessibilityLabel("Unsaved changes")
                    }
                }
                .padding(.horizontal, HideTheme.spacingMD)
                .frame(height: HideTheme.formControlHeight)
                .background(
                    row.id == selectedID ? HideTheme.accent.opacity(HideTheme.Opacity.emphasisFill) : Color.clear,
                    in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                )
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(row.id == selectedID ? .isSelected : [])
            }
        }
        .padding(HideTheme.spacingMD)
        .frame(width: HideTheme.Hint.tooltipMaxWidth)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusLarge))
        .overlay {
            RoundedRectangle(cornerRadius: HideTheme.radiusLarge).stroke(HideTheme.divider)
        }
        .accessibilityIdentifier(identifier)
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
                                    .buttonStyle(HideInteractiveButtonStyle())
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

            if let notice = model.reopenTabNotice {
                HStack(spacing: HideTheme.spacingXS) {
                    if model.core.snapshot?.recentClosed.restoring == true {
                        ProgressView()
                            .controlSize(.small)
                    } else {
                        Image(systemName: "exclamationmark.triangle")
                            .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                    }
                    Text(notice)
                        .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                        .lineLimit(1)
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
                            .buttonStyle(HideTextButtonStyle(appearance: .prominent))
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
                            .buttonStyle(HideTextButtonStyle(appearance: .prominent))
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
                HideMenuChipLabel(title: whereLabel, image: Image(systemName: "tray"))
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .accessibilityIdentifier("hide-composer-where")

            Menu {
                ForEach(model.devices) { device in
                    Button(device.label) { model.composerDeviceID = device.id }
                }
            } label: {
                HideMenuChipLabel(
                    title: model.devices.first(where: { $0.id == model.composerDeviceID })?.label
                        ?? "This Mac",
                    image: Image(systemName: "desktopcomputer")
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
                if let mark = AgentMark.image(for: provider.rawValue, side: HideTheme.agentMarkWidth) {
                    HideMenuChipLabel(
                        title: provider.rawValue.capitalized,
                        image: Image(nsImage: mark)
                    )
                } else {
                    HideMenuChipLabel(
                        title: provider.rawValue.capitalized,
                        image: Image(systemName: "sparkles")
                    )
                }
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
                .scrollContentBackground(.hidden)
                .hideInputSurface(focused: messageFocused)
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
            }
            .buttonStyle(HideTextButtonStyle(appearance: .prominent))
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
