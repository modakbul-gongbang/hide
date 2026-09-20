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
            if model.purposeUnavailableReason(for: workspace) == nil {
                PurposeEditorField(
                    label: "Purpose · optional, one line",
                    placeholder: "What is this worktree for?",
                    text: $model.worktreeDraft.purpose,
                    error: nil
                )
                .disabled(working)
                .accessibilityIdentifier("worktree-purpose")
            }
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

struct PurposeEditorField: View {
    let label: String
    let placeholder: String
    @Binding var text: String
    let error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            Text(label)
                .hideFont(size: HideTheme.Typography.body, weight: .medium)
                .foregroundStyle(HideTheme.secondary)
            HStack(spacing: HideTheme.spacingSM) {
                HideSettingsField(
                    placeholder: placeholder,
                    text: Binding(
                        get: { text },
                        set: { text = PurposeInputPresentation.normalized($0) }
                    ),
                    height: HideTheme.formControlHeight
                )
                Text(PurposeInputPresentation.countLabel(text))
                    .hideFont(size: HideTheme.Typography.caption, design: .monospaced)
                    .foregroundStyle(PurposeInputPresentation.isWarning(text) ? HideTheme.warning : HideTheme.muted)
                    .fixedSize()
            }
            if let error {
                Text(error)
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.danger)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("purpose-error")
            }
        }
    }
}

struct SetPurposeSheet: View {
    @EnvironmentObject private var model: ShellModel
    let request: CheckoutPurposeRequest

    private var working: Bool {
        model.core.snapshot?.taskOperation?.kind == "checkout_purpose"
            && model.core.snapshot?.taskOperation?.phase == "working"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                Text("Set purpose")
                    .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                Text(request.checkout.branch ?? request.checkout.label)
                    .hideFont(size: HideTheme.Typography.body)
                    .foregroundStyle(HideTheme.secondary)
            }
            PurposeEditorField(
                label: "Purpose · optional, one line",
                placeholder: "What is this checkout for?",
                text: $model.purposeText,
                error: model.purposeError
            )
            .disabled(working)
            Text("Shown on the collapsed row and the Overview header. Herdr keeps 80 characters; aim for one glance.")
                .hideFont(size: HideTheme.Typography.caption)
                .foregroundStyle(HideTheme.muted)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                Spacer()
                Button("Cancel", action: model.cancelSetPurpose)
                    .buttonStyle(HideTextButtonStyle())
                    .keyboardShortcut(.cancelAction)
                Button(working ? "Saving…" : "Save", action: model.submitSetPurpose)
                    .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                    .keyboardShortcut(.defaultAction)
                    .disabled(working)
            }
        }
        .padding(HideTheme.spacingXL)
        .frame(width: HideTheme.worktreeDialogWidth)
        .background(HideTheme.panel)
        .interactiveDismissDisabled(working)
    }
}
