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
