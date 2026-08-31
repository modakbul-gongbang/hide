import AppKit
import MarkdownUI
import SwiftUI

struct WorkbenchViewerOverlay: View {
    @EnvironmentObject private var model: ShellModel
    @State private var draft = ""
    @State private var markdownMode = "Preview"

    private var editor: CoreEditorSnapshot? { model.core.snapshot?.editor }
    private var selectedURL: URL? { editor?.path.map(URL.init(fileURLWithPath:)) }
    private var readonlyReason: String? { editor?.readonlyReason }

    var body: some View {
        Group {
            if let selectedURL {
                VStack(spacing: 0) {
                    editorToolbar(for: selectedURL)
                    Rectangle()
                        .fill(HideTheme.divider)
                        .frame(height: 1)
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
                    message: "Choose a local file from Workbench to open it here."
                )
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.background)
        .background {
            WorkbenchViewerEscapeMonitor {
                model.core.setFileViewerVisible(false)
            }
        }
        .accessibilityIdentifier("workbench-viewer-overlay")
        .task(id: editor?.path) {
            draft = editor?.contentsUTF8 ?? ""
        }
        .onChange(of: editor?.contentsUTF8) { _, contents in
            if let contents, contents != draft { draft = contents }
        }
        .onChange(of: draft) { _, value in
            guard editor?.path != nil, editor?.contentsUTF8 != value else { return }
            model.core.updateDraft(value)
        }
    }

    @ViewBuilder
    private func editorContent(for url: URL) -> some View {
        if isImage(url) {
            imagePreview(url)
        } else if url.pathExtension.lowercased() == "md", markdownMode == "Preview" {
            ScrollView {
                Markdown(draft)
                    .markdownTheme(.gitHub)
                    .textSelection(.enabled)
                    .padding(16)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        } else if editor?.contentsUTF8 != nil {
            HighlightedCodeEditor(
                text: $draft,
                language: syntaxLanguage(for: url),
                isEditable: readonlyReason == nil
            )
        } else {
            unavailable(
                title: "Preview only",
                message: editor?.readonlyReason ?? "This file type cannot be shown as text."
            )
        }
    }

    private func editorToolbar(for url: URL) -> some View {
        HStack(spacing: 10) {
            SetiFileIconView(url: url, size: 13)
            VStack(alignment: .leading, spacing: 1) {
                Text(url.lastPathComponent)
                    .hideFont(size: 12, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                Text(url.deletingLastPathComponent().path)
                    .hideFont(size: 9, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
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
                    .hideFont(size: 10, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
            }
            Spacer(minLength: 12)
            Button("Save") { model.core.saveFile(draft) }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(readonlyReason != nil || editor?.dirty != true)
                .accessibilityIdentifier("workbench-save")
            Button {
                model.core.setFileViewerVisible(false)
            } label: {
                Image(systemName: "xmark")
                    .frame(width: 18, height: 18)
            }
            .buttonStyle(.plain)
            .foregroundStyle(HideTheme.secondary)
            .help("Close viewer (Esc)")
            .accessibilityLabel("Close file viewer")
            .accessibilityIdentifier("workbench-viewer-close")
        }
        .padding(.horizontal, 14)
        .frame(height: 46)
        .background(HideTheme.panel)
    }

    private func imagePreview(_ url: URL) -> some View {
        Group {
            if let image = NSImage(contentsOf: url) {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFit()
                    .padding(18)
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
                .hideFont(size: 9, design: .monospaced)
                .foregroundStyle(HideTheme.secondary)
        }
        .hideFont(size: 10)
        .foregroundStyle(HideTheme.primary)
        .padding(10)
        .background(HideTheme.warning.opacity(0.14))
    }

    private func noticeBar(systemImage: String, message: String, color: Color) -> some View {
        Label(message, systemImage: systemImage)
            .hideFont(size: 10)
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

struct WorkbenchViewerEscapeMonitor: NSViewRepresentable {
    let onEscape: () -> Void

    static func handles(_ event: NSEvent) -> Bool {
        event.type == .keyDown && event.keyCode == 53
    }

    func makeNSView(context: Context) -> MonitorView {
        let view = MonitorView()
        view.onEscape = onEscape
        view.startMonitoring()
        return view
    }

    func updateNSView(_ nsView: MonitorView, context: Context) {
        nsView.onEscape = onEscape
        nsView.startMonitoring()
    }

    static func dismantleNSView(_ nsView: MonitorView, coordinator: ()) {
        nsView.stopMonitoring()
    }

    final class MonitorView: NSView {
        var onEscape: (() -> Void)?

        private final class Lifecycle: @unchecked Sendable {
            var monitor: Any?

            func remove() {
                if let monitor {
                    NSEvent.removeMonitor(monitor)
                    self.monitor = nil
                }
            }

            deinit {
                remove()
            }
        }

        private let lifecycle = Lifecycle()

        func startMonitoring() {
            guard lifecycle.monitor == nil else { return }
            lifecycle.monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown]) { [weak self] event in
                guard let self, WorkbenchViewerEscapeMonitor.handles(event) else { return event }
                onEscape?()
                return nil
            }
        }

        func stopMonitoring() {
            lifecycle.remove()
        }
    }
}
