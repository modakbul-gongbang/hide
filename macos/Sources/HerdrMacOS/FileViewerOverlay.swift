import AppKit
import SwiftUI

struct FileViewerOverlay: View {
    @EnvironmentObject private var model: ShellModel
    @State private var draft = ""

    private var editor: CoreEditorSnapshot? { model.core.snapshot?.editor }
    private var selectedURL: URL? { editor?.path.map(URL.init(fileURLWithPath:)) }
    private var readonlyReason: String? { editor?.readonlyReason }

    var body: some View {
        Group {
            if let selectedURL {
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
        .accessibilityIdentifier("file-viewer-overlay")
        .task(id: editor?.path) {
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
                language: syntaxLanguage(for: url),
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
                Button("Keep editing") { model.core.resolveConflict("keep_editing") }
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
        ContentUnavailableView {
            Label(title, systemImage: "doc.text.magnifyingglass")
        } description: {
            Text(message)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(ShellMetrics.panelPadding)
    }

    private func isImage(_ url: URL) -> Bool {
        ["png", "jpg", "jpeg", "gif", "webp", "tiff", "heic", "avif"].contains(url.pathExtension.lowercased())
    }

    private func syntaxLanguage(for url: URL) -> String? {
        switch url.pathExtension.lowercased() {
        case "swift": "swift"
        case "rs": "rust"
        case "js", "mjs", "cjs", "jsx": "javascript"
        case "ts", "tsx": "typescript"
        case "json", "jsonc", "jsonl": "json"
        case "md", "markdown": "markdown"
        case "sh", "bash", "zsh", "fish": "bash"
        case "toml", "ini", "cfg": "ini"
        case "py", "pyw": "python"
        case "html", "htm": "html"
        case "css", "scss", "sass", "less": "css"
        default: nil
        }
    }
}
