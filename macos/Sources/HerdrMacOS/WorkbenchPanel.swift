import SwiftUI

struct WorkbenchPanel: View {
    @EnvironmentObject private var model: ShellModel

    private var activeRoot: URL? { model.focusedPath }
    private var fontScale: CGFloat {
        CGFloat((model.core.snapshot?.uiState.fontSize ?? 13) / 13)
    }

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(
                title: "Workbench",
                systemImage: "doc.text.magnifyingglass",
                trailing: model.isRemoteContext ? "Remote" : (activeRoot?.lastPathComponent ?? "No workspace"),
                collapseAction: model.toggleRightWorkbench,
                collapseAccessibilityLabel: "Hide Workbench",
                collapseShortcut: "⌘⌥B",
                collapseAccessibilityIdentifier: "hide-toggle-right-workbench"
            )
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: 1)
            fileTree
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(HideTheme.panel)
        .task(id: "\(model.isRemoteContext)-\(activeRoot?.path ?? "")") {
            guard model.isRemoteContext, let activeRoot else { return }
            model.loadRemoteFiles(path: activeRoot.path)
        }
        .onAppear {
            if !model.isRemoteContext,
               let restored = model.core.snapshot?.uiState.selectedPath {
                model.core.openFile(URL(fileURLWithPath: restored))
            }
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
                openFile: model.core.openFile,
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
            .accessibilityIdentifier("workbench-file-tree")
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
