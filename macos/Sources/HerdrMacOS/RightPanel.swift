import SwiftUI

struct RightPanel: View {
    @EnvironmentObject private var model: ShellModel

    private var activeRoot: URL? { model.focusedPath }
    private var fontScale: CGFloat {
        CGFloat((model.core.snapshot?.uiState.fontSize ?? 13) / 13)
    }

    var body: some View {
        VStack(spacing: 0) {
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
                collapseShortcut: ShellMenuCommand.toggleRightPanel.displayShortcut,
                collapseAccessibilityIdentifier: "hide-toggle-right-panel"
            )
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: 1)
            // The summary remains shared by all sections.
            CheckoutSummaryCard()
            switch model.rightPanelSection {
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
            ContentUnavailableView {
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
            ContentUnavailableView {
                Label("Remote files unavailable", systemImage: "exclamationmark.triangle")
            } description: {
                Text(fileError)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(ShellMetrics.panelPadding)
        } else if activeRoot == nil {
            ContentUnavailableView {
                Label("Remote checkout has no path", systemImage: "externaldrive")
            } description: {
                Text(model.remote.message)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(ShellMetrics.panelPadding)
        } else if model.remote.fileState == "loading" {
            VStack(spacing: 8) {
                ProgressView()
                    .controlSize(.small)
                    .tint(HideTheme.secondary)
                Text("Loading remote files")
                    .hideFont(size: 10)
                    .foregroundStyle(HideTheme.muted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("remote-files-loading")
        } else if model.remote.files.isEmpty {
            ContentUnavailableView {
                Label("No remote files", systemImage: "folder")
            } description: {
                Text("The selected remote checkout has no visible top-level files.")
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(ShellMetrics.panelPadding)
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 1) {
                    ForEach(model.remote.files) { node in
                        HStack(spacing: 6) {
                            if node.isDirectory {
                                Image(systemName: "folder")
                                    .font(.system(size: 11))
                                    .foregroundStyle(HideTheme.secondary)
                                    .frame(width: 16, height: 16)
                            } else {
                                SetiFileIconView(url: URL(fileURLWithPath: node.path))
                            }
                            Text(node.name)
                                .hideFont(size: 11)
                                .foregroundStyle(HideTheme.primary)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                        }
                        .padding(.horizontal, 10)
                        .frame(height: 22)
                        .contentShape(Rectangle())
                        .accessibilityIdentifier("remote-workspace-item-\(node.path)")
                    }
                }
                .padding(.vertical, 6)
            }
            .overlay(alignment: .topTrailing) {
                Text("Remote, read-only")
                    .hideFont(size: 9, weight: .medium)
                    .foregroundStyle(HideTheme.muted)
                    .padding(8)
            }
        }
    }
}

/// The right panel's changes section: the checkout's changed files, and the
/// diff of whichever one is selected. Read-only by decision - staging and
/// committing stay in the terminal for this round.
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
            VSplitView {
                changedFileList
                diffPane
            }
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
            LazyVStack(alignment: .leading, spacing: 1) {
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
            model.selectChangedFile(isSelected ? nil : entry.path, committed: committed)
        }
    }

    @ViewBuilder
    private var diffPane: some View {
        if let selected = changes.selectedPath {
            if let diff = changes.diff, diff.path == selected {
                DiffText(diff: diff)
            } else {
                // The selection is applied at once and the diff arrives on the
                // next read, so the wait says so rather than showing an empty
                // pane that reads as "no difference".
                VStack(spacing: HideTheme.spacingSM) {
                    ProgressView()
                        .controlSize(.small)
                        .tint(HideTheme.secondary)
                    Text("Reading the diff")
                        .hideFont(size: 10)
                        .foregroundStyle(HideTheme.muted)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("changes-diff-loading")
            }
        } else {
            Text("Select a file to see its diff.")
                .hideFont(size: 11)
                .foregroundStyle(HideTheme.muted)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("changes-diff-empty")
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
                    .hideFont(size: 8, weight: .semibold)
                    .foregroundStyle(HideTheme.muted)
                    .frame(width: 10)
                Text(title)
                    .hideFont(size: 9, weight: .semibold)
                    .foregroundStyle(HideTheme.secondary)
                Text("\(count)")
                    .hideFont(size: 9, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
                if let detail {
                    Text("→ \(detail)")
                        .hideFont(size: 9, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .frame(height: 20)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
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
                    .hideFont(size: 11)
                    .foregroundStyle(HideTheme.primary)
                    .lineLimit(1)
                if !entry.directory.isEmpty {
                    // The directory is context, not identity, so it is dimmer
                    // and it is the half that gets truncated.
                    Text(entry.directory)
                        .hideFont(size: 10)
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
                    .hideFont(size: 9, design: .monospaced)
                }
                Text(entry.status.badge)
                    .hideFont(size: 10, weight: .medium)
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
        .buttonStyle(.plain)
        .help(entry.relativePath)
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

/// A unified diff, tinted by line kind. Read-only text rather than the code
/// editor: a diff is not one file's language, and the editor's highlighter
/// would colour it as whatever the file happens to be.
private struct DiffText: View {
    let diff: CoreChangedFileDiff

    var body: some View {
        ScrollView([.vertical, .horizontal]) {
            VStack(alignment: .leading, spacing: 0) {
                if let reason = diff.notice {
                    Text(reason)
                        .hideFont(size: 10)
                        .foregroundStyle(HideTheme.warning)
                        .padding(.horizontal, HideTheme.spacingMD)
                        .padding(.vertical, HideTheme.spacingXS)
                }
                ForEach(Array(diff.text.split(separator: "\n", omittingEmptySubsequences: false).enumerated()), id: \.offset) { line in
                    DiffLine(text: String(line.element))
                }
            }
            .padding(.vertical, HideTheme.spacingXS)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier("changes-diff-text")
    }
}

private struct DiffLine: View {
    let text: String

    var body: some View {
        Text(text.isEmpty ? " " : text)
            .font(.system(size: HideTheme.editorBaseFontSize, design: .monospaced))
            .foregroundStyle(foreground)
            .lineLimit(1)
            .fixedSize(horizontal: true, vertical: false)
            .padding(.horizontal, HideTheme.spacingMD)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(background)
    }

    /// Header lines (`+++`, `---`) are not additions or removals and are
    /// checked before the single-character prefixes.
    private var kind: DiffLineKind { DiffLineKind.of(text) }

    private var foreground: Color {
        switch kind {
        case .added: HideTheme.diffAdded
        case .removed: HideTheme.diffRemoved
        case .hunk: HideTheme.secondary
        case .context: HideTheme.primary
        }
    }

    private var background: Color {
        switch kind {
        case .added: HideTheme.diffAddedBackground
        case .removed: HideTheme.diffRemovedBackground
        case .hunk, .context: Color.clear
        }
    }
}

enum DiffLineKind {
    case added
    case removed
    case hunk
    case context

    static func of(_ line: String) -> DiffLineKind {
        if line.hasPrefix("+++") || line.hasPrefix("---") { return .hunk }
        if line.hasPrefix("@@") || line.hasPrefix("diff ") || line.hasPrefix("index ") {
            return .hunk
        }
        if line.hasPrefix("+") { return .added }
        if line.hasPrefix("-") { return .removed }
        return .context
    }
}

private struct ChangesNotice: View {
    let title: String
    let systemImage: String
    let message: String

    var body: some View {
        ContentUnavailableView {
            Label(title, systemImage: systemImage)
        } description: {
            Text(message)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(ShellMetrics.panelPadding)
        .accessibilityIdentifier("changes-notice")
    }
}
