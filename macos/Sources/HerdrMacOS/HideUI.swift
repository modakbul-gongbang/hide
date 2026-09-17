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
