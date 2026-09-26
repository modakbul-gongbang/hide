import SwiftUI

struct ShellView: View {
    @EnvironmentObject private var model: ShellModel
    /// The window's content height, read for the Settings sheet, which sizes
    /// itself to the window it is presented over.
    @State private var contentHeight: CGFloat?

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
        .background(
            GeometryReader { proxy in
                Color.clear
                    .onAppear { contentHeight = proxy.size.height }
                    .onChange(of: proxy.size.height) { _, height in contentHeight = height }
            }
        )
        .hideOverlayHost()
        .preferredColorScheme(.dark)
        .environment(\.colorScheme, .dark)
        .environment(\.hideAccent, HideTheme.color(for: model.core.snapshot?.uiState.accentHex ?? "#B9FF66"))
        .environment(\.hideFontScale, CGFloat((model.core.snapshot?.uiState.fontSize ?? 13) / 13))
        .tint(HideTheme.color(for: model.core.snapshot?.uiState.accentHex ?? "#B9FF66"))
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
                initialTab: model.settingsInitialTab,
                availableHeight: contentHeight
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
        .sheet(item: $model.purposeRequest) { request in
            SetPurposeSheet(request: request)
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
            // The counts are read at presentation from the current row, so a
            // pane that opened or closed since the menu click is counted.
            let current = model.workspaces.first { $0.id == workspace.id } ?? workspace
            let prompt = WorkspaceRemovalPrompt(label: current.label, removal: current.removal)
            return Alert(
                title: Text(prompt.title),
                message: Text(prompt.message),
                primaryButton: .destructive(Text(prompt.confirmLabel), action: model.confirmRemoveWorkspace),
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
            if model.interactionStatusRefreshAvailable {
                Button("Check status", action: model.refreshInteractionStatus)
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("hide-refresh-activity-status")
            }
            Button("OK", action: model.clearInteractionNotice)
        } message: {
            Text(model.interactionNotice ?? "")
        }
        // A close with a consequence - a working agent - waits here for the
        // operator's answer.
        // The model holds the pending target, so the header X and ⌘W share
        // one prompt. The destructive action is explicit, while Escape and
        // the cancel action keep the pane open.
        .alert(
            model.consequenceNotice?.title ?? "",
            isPresented: Binding(
                get: { model.consequenceNotice != nil },
                set: { if !$0, model.consequenceNotice != nil { model.cancelConsequencePreview() } }
            ),
            presenting: model.consequenceNotice
        ) { _ in
            Button("Keep open", role: .cancel, action: model.cancelConsequencePreview)
                .keyboardShortcut(.defaultAction)
            Button("Stop work and close", role: .destructive, action: model.confirmConsequencePreview)
        } message: { notice in
            Text(notice.message)
        }
    }
}
