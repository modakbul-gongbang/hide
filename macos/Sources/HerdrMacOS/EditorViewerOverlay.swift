import AppKit
import SwiftUI

struct EditorViewerOverlay: View {
    @EnvironmentObject private var model: ShellModel
    @State private var draft = ""

    private var editor: CoreEditorSnapshot? { model.core.snapshot?.editor }
    private var activeTab: CoreEditorTabSnapshot? {
        guard let activeID = editor?.activeTabID else { return nil }
        return editor?.tabs.first(where: { $0.id == activeID })
    }
    private var selectedURL: URL? { editor?.path.map(URL.init(fileURLWithPath:)) }
    private var readonlyReason: String? { editor?.readonlyReason }

    var body: some View {
        Group {
            if let activeTab, activeTab.kind == .diff {
                diffViewer(tab: activeTab)
            } else if let selectedURL {
                VStack(spacing: HideTheme.spacingNone) {
                    editorContent(for: selectedURL)
                    if let conflict = editor?.conflict {
                        conflictBar(conflict)
                    } else if let readonlyReason, editor?.contentsUTF8 != nil {
                        noticeBar(systemImage: "lock.fill", message: readonlyReason, color: HideTheme.warning)
                    }
                }
            } else {
                unavailable(
                    title: "No file selected",
                    message: "Choose a local file from the explorer to open it here."
                )
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.background)
        .accessibilityIdentifier("editor-viewer-overlay")
        .task(id: editor?.activeTabID) {
            draft = editor?.contentsUTF8 ?? ""
        }
        .onChange(of: editor?.contentsUTF8) { _, contents in
            if let contents, contents != draft { draft = contents }
        }
        .onChange(of: draft) { _, value in
            guard editor?.path != nil, editor?.contentsUTF8 != value else { return }
            model.core.updateDraft(value)
            model.core.scheduleFileSave(value)
        }
    }

    @ViewBuilder
    private func editorContent(for url: URL) -> some View {
        if isImage(url) {
            imagePreview(url)
        } else if editor?.contentsUTF8 != nil {
            HighlightedCodeEditor(
                text: $draft,
                language: editor?.language,
                isEditable: readonlyReason == nil,
                textScale: model.editorTextScale
            )
        } else {
            unavailable(
                title: "Preview only",
                message: editor?.readonlyReason ?? "This file type cannot be shown as text."
            )
        }
    }

    @ViewBuilder
    private func diffViewer(tab: CoreEditorTabSnapshot) -> some View {
        let changes = model.changes
        if let reason = changes.unavailableReason {
            unavailable(title: "Diff unavailable", message: reason)
        } else if changes.selectedPath != tab.path
                    || changes.selectedCommitted != tab.diffCommitted {
            unavailable(
                title: "Diff unavailable",
                message: "This file is no longer in the selected Changes group."
            )
        } else if let diff = changes.diff, diff.path == tab.path {
            DiffText(diff: diff)
        } else {
            VStack(spacing: HideTheme.spacingSM) {
                ProgressView()
                    .controlSize(.small)
                    .tint(HideTheme.secondary)
                Text("Reading the diff")
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.muted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("changes-diff-loading")
        }
    }

    private func imagePreview(_ url: URL) -> some View {
        Group {
            if let image = NSImage(contentsOf: url) {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFit()
                    .padding(HideTheme.spacingLG)
            } else {
                unavailable(title: "Image unavailable", message: "The image could not be decoded.")
            }
        }
    }

    private func conflictBar(_ conflict: CoreEditorConflict) -> some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            Label("This file changed on disk. Your draft is preserved.", systemImage: "exclamationmark.triangle.fill")
            HStack {
                Button("Reload disk version") { model.core.resolveConflict("reload") }
                    .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                Button("Keep editing") { model.core.resolveConflict("keep_editing") }
                    .buttonStyle(HideTextButtonStyle(appearance: .quiet))
            }
            Text("Opened \(conflict.openedModifiedAt), disk \(conflict.diskModifiedAt)")
                .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                .foregroundStyle(HideTheme.secondary)
        }
        .hideFont(size: HideTheme.Typography.caption)
        .foregroundStyle(HideTheme.primary)
        .padding(HideTheme.spacingMD)
        .background(HideTheme.warning.opacity(HideTheme.Opacity.selectedFill))
    }

    private func noticeBar(systemImage: String, message: String, color: Color) -> some View {
        Label(message, systemImage: systemImage)
            .hideFont(size: HideTheme.Typography.caption)
            .foregroundStyle(color)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(HideTheme.spacingMD)
            .background(color.opacity(HideTheme.Opacity.subtleFill))
    }

    private func unavailable(title: String, message: String) -> some View {
        HideEmptyState(
            title,
            systemImage: "doc.text.magnifyingglass",
            description: Text(message)
        )
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(ShellMetrics.panelPadding)
    }

    private func isImage(_ url: URL) -> Bool {
        ["png", "jpg", "jpeg", "gif", "webp", "tiff", "heic", "avif"].contains(url.pathExtension.lowercased())
    }

}
