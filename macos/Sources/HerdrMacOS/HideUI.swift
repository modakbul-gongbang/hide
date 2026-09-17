import AppKit
import Foundation
import SwiftUI



struct WorktreeCreationSheet: View {
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

struct PetDashboardView: View {
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
