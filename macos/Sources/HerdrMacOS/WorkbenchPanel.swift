import AppKit
import MarkdownUI
import SwiftUI

struct WorkspaceFileNode: Identifiable, Hashable {
    let url: URL
    let isDirectory: Bool
    let children: [WorkspaceFileNode]?

    var id: String { url.path }
    var name: String { url.lastPathComponent }
}

enum WorkspaceTree {
    private static let skippedNames: Set<String> = [
        ".git", ".build", "build", "target", "DerivedData",
    ]

    static func load(root: URL) -> [WorkspaceFileNode] {
        [node(at: root, depth: 0)].compactMap { $0 }
    }

    private static func node(at url: URL, depth: Int) -> WorkspaceFileNode? {
        guard depth <= 12 else { return nil }
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory) else {
            return nil
        }
        guard isDirectory.boolValue else {
            return WorkspaceFileNode(url: url, isDirectory: false, children: nil)
        }
        let children = (try? FileManager.default.contentsOfDirectory(
            at: url,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]
        ))?
            .filter { !skippedNames.contains($0.lastPathComponent) }
            .sorted { lhs, rhs in
                let lhsDirectory = (try? lhs.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) ?? false
                let rhsDirectory = (try? rhs.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) ?? false
                if lhsDirectory != rhsDirectory { return lhsDirectory }
                return lhs.lastPathComponent.localizedStandardCompare(rhs.lastPathComponent) == .orderedAscending
            }
            .compactMap { node(at: $0, depth: depth + 1) } ?? []
        return WorkspaceFileNode(url: url, isDirectory: true, children: children)
    }
}

struct WorkbenchPanel: View {
    @EnvironmentObject private var model: ShellModel
    @State private var roots: [WorkspaceFileNode] = []
    @State private var draft = ""
    @State private var markdownMode = "Preview"

    private var editor: CoreEditorSnapshot? { model.core.snapshot?.editor }
    private var selectedURL: URL? { editor?.path.map(URL.init(fileURLWithPath:)) }

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(
                title: "Workbench",
                systemImage: "doc.text.magnifyingglass",
                trailing: selectedURL?.lastPathComponent ?? "No file selected"
            )
            Divider()
            VStack(spacing: 0) {
                fileTree
                    .frame(minHeight: 150, idealHeight: 230)
                Divider()
                viewer
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(Color(nsColor: .controlBackgroundColor))
        .onAppear {
            roots = WorkspaceTree.load(root: model.core.workspaceRoot)
            if let restored = model.core.snapshot?.uiState.selectedPath {
                model.core.openFile(URL(fileURLWithPath: restored))
            }
        }
        .onChange(of: editor?.contentsUTF8) { _, contents in
            if let contents, contents != draft { draft = contents }
        }
        .onChange(of: draft) { _, value in
            guard editor?.path != nil else { return }
            model.core.updateDraft(value)
        }
    }

    private var fileTree: some View {
        ScrollView {
            OutlineGroup(roots, children: \.children) { node in
                Button {
                    if node.isDirectory {
                        let current = Set(model.core.snapshot?.uiState.expandedPaths ?? [])
                        var updated = current
                        if !updated.insert(node.url.path).inserted { updated.remove(node.url.path) }
                        model.core.persistUIState(expandedPaths: updated.sorted())
                    } else {
                        model.core.openFile(node.url)
                    }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: node.isDirectory ? "folder" : icon(for: node.url))
                            .foregroundStyle(node.isDirectory ? .blue : .secondary)
                        Text(node.name)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("workspace-file-\(node.name)")
            }
            .padding(10)
        }
        .overlay(alignment: .topTrailing) {
            Text("Local, existing files only")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .padding(8)
        }
        .accessibilityIdentifier("workbench-file-tree")
    }

    @ViewBuilder
    private var viewer: some View {
        if let selectedURL {
            VStack(spacing: 0) {
                editorToolbar(for: selectedURL)
                Divider()
                if isImage(selectedURL) {
                    imagePreview(selectedURL)
                } else if selectedURL.pathExtension.lowercased() == "md", markdownMode == "Preview" {
                    ScrollView {
                        Markdown(draft)
                            .markdownTheme(.gitHub)
                            .textSelection(.enabled)
                            .padding(12)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                } else if editor?.contentsUTF8 != nil {
                    HighlightedCodeEditor(
                        text: $draft,
                        language: syntaxLanguage(for: selectedURL),
                        isEditable: editor?.readonlyReason == nil
                    )
                } else {
                    unavailable(
                        title: "Preview only",
                        message: editor?.readonlyReason ?? "This file type cannot be shown as text."
                    )
                }
                if let conflict = editor?.conflict {
                    conflictBar(conflict)
                } else if let reason = editor?.readonlyReason, editor?.contentsUTF8 != nil {
                    noticeBar(systemImage: "lock.fill", message: reason, color: .orange)
                }
            }
        } else {
            unavailable(
                title: "Choose a file",
                message: "Browse the local tree above to preview or edit an existing file."
            )
        }
    }

    private func editorToolbar(for url: URL) -> some View {
        HStack(spacing: 8) {
            if url.pathExtension.lowercased() == "md" {
                Picker("Markdown mode", selection: $markdownMode) {
                    Text("Preview").tag("Preview")
                    Text("Edit").tag("Edit")
                }
                .pickerStyle(.segmented)
                .frame(width: 140)
            }
            if let diff = editor?.diff,
               !diff.addedLines.isEmpty || !diff.removedLines.isEmpty {
                Label("+\(diff.addedLines.count) -\(diff.removedLines.count)", systemImage: "arrow.left.arrow.right")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button("Save") { model.core.saveFile(draft) }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(editor?.readonlyReason != nil || editor?.dirty != true)
                .accessibilityIdentifier("workbench-save")
        }
        .padding(.horizontal, 10)
        .frame(height: 38)
    }

    private func imagePreview(_ url: URL) -> some View {
        Group {
            if let image = NSImage(contentsOf: url) {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFit()
                    .padding(14)
            } else {
                unavailable(title: "Image unavailable", message: "The image could not be decoded.")
            }
        }
    }

    private func conflictBar(_ conflict: CoreEditorConflict) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("This file changed on disk. Your draft is preserved.", systemImage: "exclamationmark.triangle.fill")
            HStack {
                Button("Reload disk version") { model.core.resolveConflict("reload") }
                Button("Keep editing") { model.core.resolveConflict("keep_editing") }
            }
            Text("Opened \(conflict.openedModifiedAt), disk \(conflict.diskModifiedAt)")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .font(.caption)
        .padding(10)
        .background(Color.orange.opacity(0.14))
    }

    private func noticeBar(systemImage: String, message: String, color: Color) -> some View {
        Label(message, systemImage: systemImage)
            .font(.caption)
            .foregroundStyle(color)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(10)
            .background(color.opacity(0.1))
    }

    private func unavailable(title: String, message: String) -> some View {
        ContentUnavailableView {
            Label(title, systemImage: "doc.text.magnifyingglass")
        } description: {
            Text(message)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(ShellMetrics.panelPadding)
    }

    private func isImage(_ url: URL) -> Bool {
        ["png", "jpg", "jpeg", "gif", "webp", "tiff", "heic"].contains(url.pathExtension.lowercased())
    }

    private func icon(for url: URL) -> String {
        if isImage(url) { return "photo" }
        if url.pathExtension.lowercased() == "md" { return "text.book.closed" }
        return "doc.text"
    }

    private func syntaxLanguage(for url: URL) -> String? {
        switch url.pathExtension.lowercased() {
        case "swift": "swift"
        case "rs": "rust"
        case "js": "javascript"
        case "ts": "typescript"
        case "json": "json"
        case "md": "markdown"
        case "sh", "zsh": "bash"
        case "toml": "ini"
        default: nil
        }
    }
}
