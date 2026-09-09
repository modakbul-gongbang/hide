import SwiftUI

/// Checkboxes grant permission for exact reviewed folders, never registrations or branches.
struct MergedWorktreeCleanup: View {
    @EnvironmentObject private var model: ShellModel
    let review: CoreCleanup?
    let close: () -> Void
    @State private var selected = Set<String>()
    private var busy: Bool { review == nil || review?.phase == "loading" || review?.phase == "removing" }
    private var eligible: Set<String> { Set(review?.rows.filter { $0.exclusion == nil }.map(\.path) ?? []) }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            Text("Clean up merged worktrees").hideFont(size: HideTheme.Typography.title, weight: .semibold)
            Text("Select folders to permanently remove. Only clean, unused worktrees merged into local main are eligible. Branches and Git history stay.")
                .foregroundStyle(HideTheme.secondary).fixedSize(horizontal: false, vertical: true)
            if busy {
                ProgressView(review?.phase == "removing" ? "Rechecking and removing selected folders…" : "Checking Git, panes and allocated disk…")
            }
            if let message = review?.message { Text(message).foregroundStyle(HideTheme.warning) }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: HideTheme.spacingMD) {
                    ForEach([true, false], id: \.self) { available in
                        let rows = review?.rows.filter { ($0.exclusion == nil) == available } ?? []
                        if !busy && review?.phase != "failed" {
                            Text(available ? "Available · \(rows.count)" : "Excluded · \(rows.count)")
                                .fontWeight(.semibold).foregroundStyle(HideTheme.secondary)
                            if available && rows.isEmpty { Text("No worktrees are eligible for cleanup.").foregroundStyle(HideTheme.secondary) }
                        }
                    ForEach(rows) { row in
                        HStack(alignment: .top, spacing: HideTheme.spacingSM) {
                            Toggle(isOn: Binding(get: { selected.contains(row.path) }, set: { value in
                                if value { selected.insert(row.path) } else { selected.remove(row.path) }
                            })) { EmptyView() }.toggleStyle(HideCheckboxStyle()).labelsHidden()
                                .disabled(busy || row.exclusion != nil || review?.phase != "review")
                                .accessibilityLabel("Remove \(row.branch ?? "detached") at \(row.path)")
                            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                                HStack(alignment: .firstTextBaseline) {
                                    Text(row.branch ?? "Detached HEAD").fontWeight(.semibold)
                                    Spacer(minLength: HideTheme.spacingSM)
                                    Text(row.disk.totalBytes.map(CheckoutCardPresentation.formattedBytes) ?? "Unavailable")
                                        .foregroundStyle(HideTheme.secondary)
                                }
                                Text(row.path).foregroundStyle(HideTheme.secondary).textSelection(.enabled)
                                    .fixedSize(horizontal: false, vertical: true)
                                if let reason = row.message ?? row.exclusion {
                                    Label(reason, systemImage: row.result == "removed" ? "checkmark.circle" : "info.circle")
                                        .foregroundStyle(row.result == "refused" ? HideTheme.warning : HideTheme.secondary)
                                        .fixedSize(horizontal: false, vertical: true)
                                }
                                if let reason = row.disk.unavailableReason {
                                    Text(reason).foregroundStyle(HideTheme.secondary)
                                }
                            }
                        }
                    }
                    }
                }
            }
            Text("Sizes are allocated on disk, not guaranteed reclaimed space. State is checked again before removal.")
                .hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.muted)
            HStack {
                Button(review?.phase == "complete" ? "Done" : "Cancel", action: close).disabled(review?.phase == "removing")
                    .keyboardShortcut(.cancelAction)
                Spacer()
                if !busy && review?.phase != "review" {
                    Button("Review again") { selected.removeAll(); model.core.dispatch(kind: "cleanup_review", payload: [:]) }
                }
                Button("Remove \(selected.intersection(eligible).count) \(selected.intersection(eligible).count == 1 ? "folder" : "folders")", role: .destructive) {
                    guard let review else { return }
                    model.core.dispatch(kind: "cleanup_confirm", payload: ["id": review.id, "paths": selected.intersection(eligible).sorted()])
                }.disabled(busy || review?.phase != "review" || selected.intersection(eligible).isEmpty)
            }
        }.buttonStyle(HideTextButtonStyle(density: .regular))
            .hideFont(size: HideTheme.Typography.body).foregroundStyle(HideTheme.primary)
            .padding(HideTheme.spacingXL).frame(width: HideTheme.worktreeDialogWidth, height: HideTheme.Overview.cleanupHeight)
            .background(HideTheme.background).hideOverlayHost()
            .interactiveDismissDisabled(review?.phase == "removing")
            .onChange(of: review?.id) { _, _ in selected.removeAll() }
            .accessibilityIdentifier("merged-worktree-cleanup")
    }
}
