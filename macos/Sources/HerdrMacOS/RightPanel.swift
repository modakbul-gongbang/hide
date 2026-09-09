import SwiftUI

struct RightPanel: View {
    @EnvironmentObject private var model: ShellModel

    private var activeRoot: URL? { model.focusedPath }
    private var fontScale: CGFloat {
        CGFloat((model.core.snapshot?.uiState.fontSize ?? 13) / 13)
    }

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            PanelHeader(
                title: model.rightPanelSection.title,
                systemImage: model.rightPanelSection.systemImage,
                trailing: model.isRemoteContext ? "Remote" : (activeRoot?.lastPathComponent ?? "No workspace"),
                sections: PanelHeader.Sections(
                    active: model.rightPanelSection,
                    select: model.selectRightPanelSection
                ),
                collapseAction: model.toggleRightPanel,
                collapseAccessibilityLabel: "Hide Right Panel",
                collapseCommand: .menu(.toggleRightPanel),
                collapseAccessibilityIdentifier: "hide-toggle-right-panel"
            )
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: 1)
            switch model.rightPanelSection {
            case .overview:
                CheckoutOverview()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .explorer:
                fileTree
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .git:
                GitWorktreesView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .changes:
                ChangesView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(HideTheme.panel)
        .task(id: "\(model.isRemoteContext)-\(activeRoot?.path ?? "")") {
            guard model.isRemoteContext, let activeRoot else { return }
            model.loadRemoteFiles(path: activeRoot.path)
        }
    }

    @ViewBuilder
    private var fileTree: some View {
        if model.isRemoteContext {
            remoteFileTree
        } else if let activeRoot {
            WorkspaceOutlineView(
                rootURL: activeRoot,
                expandedPaths: Set(model.core.snapshot?.uiState.expandedPaths ?? []),
                selectedPath: model.core.snapshot?.uiState.selectedPath,
                fontScale: fontScale,
                openFile: model.openFile,
                updateExpandedPaths: { paths in
                    model.core.persistUIState(expandedPaths: paths)
                }
            )
        } else {
            HideEmptyState {
                Label("No workspace", systemImage: "folder")
            } description: {
                Text("Choose New Workspace to browse local files.")
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(ShellMetrics.panelPadding)
            .accessibilityIdentifier("explorer-file-tree")
        }
    }

    @ViewBuilder
    private var remoteFileTree: some View {
        if let fileError = model.remote.fileError {
            HideEmptyState {
                Label("Remote files unavailable", systemImage: "exclamationmark.triangle")
            } description: {
                Text(fileError)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(ShellMetrics.panelPadding)
        } else if activeRoot == nil {
            HideEmptyState {
                Label("Remote checkout has no path", systemImage: "externaldrive")
            } description: {
                Text(model.remote.message)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(ShellMetrics.panelPadding)
        } else if model.remote.fileState == "loading" {
            VStack(spacing: HideTheme.spacingSM) {
                ProgressView()
                    .controlSize(.small)
                    .tint(HideTheme.secondary)
                Text("Loading remote files")
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.muted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("remote-files-loading")
        } else if model.remote.files.isEmpty {
            HideEmptyState {
                Label("No remote files", systemImage: "folder")
            } description: {
                Text("The selected remote checkout has no visible top-level files.")
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(ShellMetrics.panelPadding)
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    ForEach(model.remote.files) { node in
                        HStack(spacing: HideTheme.spacingSM) {
                            if node.isDirectory {
                                Image(systemName: "folder")
                                    .hideFont(size: HideTheme.Typography.body)
                                    .foregroundStyle(HideTheme.secondary)
                                    .frame(width: 16, height: 16)
                            } else {
                                SetiFileIconView(url: URL(fileURLWithPath: node.path))
                            }
                            Text(node.name)
                                .hideFont(size: HideTheme.Typography.body)
                                .foregroundStyle(HideTheme.primary)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                        }
                        .padding(.horizontal, HideTheme.spacingMD)
                        .frame(height: 22)
                        .contentShape(Rectangle())
                        .accessibilityIdentifier("remote-workspace-item-\(node.path)")
                    }
                }
                .padding(.vertical, HideTheme.spacingSM)
            }
            .overlay(alignment: .topTrailing) {
                Text("Remote, read-only")
                    .hideFont(size: HideTheme.Typography.micro, weight: .medium)
                    .foregroundStyle(HideTheme.muted)
                    .padding(HideTheme.spacingSM)
            }
        }
    }
}

/// The right panel's changes section is the checkout's changed-file list.
/// Activating a row opens its read-only diff in the central editor strip;
/// staging and committing stay in the terminal for this round.
struct ChangesView: View {
    @EnvironmentObject private var model: ShellModel
    @State private var uncommittedExpanded = true
    @State private var committedExpanded = true

    private var changes: CoreChangesSnapshot { model.changes }

    var body: some View {
        if model.isRemoteContext {
            ChangesNotice(
                title: "Changes are local only",
                systemImage: "externaldrive",
                message: "Git is read on this Mac, so a remote checkout has no changes view."
            )
        } else if let reason = changes.unavailableReason {
            ChangesNotice(
                title: "Changes unavailable",
                systemImage: "exclamationmark.triangle",
                message: reason
            )
        } else if changes.rootPath == nil {
            ChangesNotice(
                title: "No workspace",
                systemImage: "folder",
                message: "Choose New Workspace to see what changed."
            )
        } else if changes.entries.isEmpty && changes.committed.isEmpty {
            ChangesNotice(
                title: "No changes",
                systemImage: "checkmark.circle",
                message: "This checkout matches its last commit."
            )
        } else {
            changedFileList
            .accessibilityIdentifier("changes-view")
        }
    }

    /// Two groups: what is not saved yet, and what this branch is.
    ///
    /// They answer different questions, which is why one flat list was the
    /// wrong shape - a file edited but not committed and a file this branch
    /// added are both "changed" and mean nothing alike. The committed group
    /// is absent, not empty, when there is no base branch to compare against:
    /// an empty group would claim the branch has no commits.
    private var changedFileList: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                if !changes.entries.isEmpty {
                    ChangesGroupHeader(
                        title: "UNCOMMITTED",
                        count: changes.entries.count,
                        isExpanded: $uncommittedExpanded
                    )
                    if uncommittedExpanded {
                        ForEach(changes.entries, id: \.uncommittedRowID) { entry in
                            row(entry, committed: false)
                        }
                    }
                }
                if let base = changes.baseBranch, !changes.committed.isEmpty {
                    ChangesGroupHeader(
                        title: "COMMITTED ON BRANCH",
                        count: changes.committed.count,
                        detail: base,
                        isExpanded: $committedExpanded
                    )
                    if committedExpanded {
                        // Identity carries the group: the same path is in both
                        // groups whenever a committed file is edited again,
                        // and two rows with one identity collapse into one.
                        ForEach(changes.committed, id: \.committedRowID) { entry in
                            row(entry, committed: true)
                        }
                    }
                }
            }
            .padding(.vertical, HideTheme.spacingXS)
        }
        .frame(minHeight: 80)
    }

    private func row(_ entry: CoreChangedFile, committed: Bool) -> some View {
        let isSelected = entry.path == changes.selectedPath
            && changes.selectedCommitted == committed
        return ChangedFileRow(entry: entry, committed: committed, isSelected: isSelected) {
            model.selectChangedFile(entry.path, committed: committed)
        }
    }
}

/// One group's header: its name, how many files are in it, and a disclosure.
private struct ChangesGroupHeader: View {
    let title: String
    let count: Int
    var detail: String?
    @Binding var isExpanded: Bool

    var body: some View {
        Button { isExpanded.toggle() } label: {
            HStack(spacing: HideTheme.spacingXS) {
                Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                    .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                    .foregroundStyle(HideTheme.muted)
                    .frame(width: 10)
                Text(title)
                    .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                Text("\(count)")
                    .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
                if let detail {
                    Text("→ \(detail)")
                        .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .frame(height: 20)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .accessibilityIdentifier("changes-group-\(title.lowercased().replacingOccurrences(of: " ", with: "-"))")
        .accessibilityLabel("\(title), \(count) files")
        .accessibilityValue(isExpanded ? "Expanded" : "Collapsed")
    }
}

private struct ChangedFileRow: View {
    let entry: CoreChangedFile
    let committed: Bool
    let isSelected: Bool
    let activate: () -> Void

    var body: some View {
        Button(action: activate) {
            HStack(spacing: HideTheme.spacingSM) {
                SetiFileIconView(url: URL(fileURLWithPath: entry.path))
                Text(entry.name)
                    .hideFont(size: HideTheme.Typography.body)
                    .foregroundStyle(HideTheme.primary)
                    .lineLimit(1)
                if !entry.directory.isEmpty {
                    // The directory is context, not identity, so it is dimmer
                    // and it is the half that gets truncated.
                    Text(entry.directory)
                        .hideFont(size: HideTheme.Typography.caption)
                        .foregroundStyle(HideTheme.muted)
                        .lineLimit(1)
                        .truncationMode(.head)
                }
                Spacer(minLength: HideTheme.spacingXS)
                if let added = entry.addedLines, let removed = entry.removedLines {
                    HStack(spacing: HideTheme.spacingXS) {
                        Text("+\(added)").foregroundStyle(HideTheme.diffAdded)
                        Text("-\(removed)").foregroundStyle(HideTheme.diffRemoved)
                    }
                    .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                }
                Text(entry.status.badge)
                    .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                    .foregroundStyle(statusColor)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .frame(height: 22)
            .contentShape(Rectangle())
            .background(
                RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                    .fill(isSelected ? HideTheme.elevated : Color.clear)
                    .padding(.horizontal, HideTheme.spacingXS)
            )
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .hideTooltip(entry.relativePath)
        .accessibilityIdentifier(committed ? "committed-file-\(entry.relativePath)" : "changed-file-\(entry.relativePath)")
        .accessibilityLabel(accessibilityLabel)
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    private var statusColor: Color {
        switch entry.status {
        case .added, .untracked: HideTheme.diffAdded
        case .deleted: HideTheme.diffRemoved
        case .modified: HideTheme.warning
        }
    }

    /// The letter and the two numbers said in words, because the row shows
    /// them as a letter and two numbers.
    private var accessibilityLabel: String {
        var parts = [entry.relativePath, entry.status.rawValue]
        if let added = entry.addedLines, let removed = entry.removedLines {
            parts.append("\(added) lines added, \(removed) removed")
        }
        return parts.joined(separator: ", ")
    }
}

private struct ChangesNotice: View {
    let title: String
    let systemImage: String
    let message: String

    var body: some View {
        HideEmptyState {
            Label(title, systemImage: systemImage)
        } description: {
            Text(message)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(ShellMetrics.panelPadding)
        .accessibilityIdentifier("changes-notice")
    }
}
