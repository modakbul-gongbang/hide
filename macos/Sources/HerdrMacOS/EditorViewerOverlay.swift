import AppKit
import SwiftUI

struct EditorViewerOverlay: View {
    @EnvironmentObject private var model: ShellModel
    @State private var draft = ""
    @State private var draftTabID: String?
    @State private var findRequest = 0
    @State private var notice: String?

    private var isMarkdown: Bool { editor?.language == "markdown" || ["md", "markdown", "mdown"].contains(selectedURL?.pathExtension.lowercased() ?? "") }
    private var preview: Bool { activeTab?.markdownPreview ?? true }
    private var wrapsLines: Bool { activeTab?.wrap ?? false }
    private var currentDraft: String { draftTabID == editor?.activeTabID ? draft : (editor?.contentsUTF8 ?? "") }
    private var draftBinding: Binding<String> {
        let target = editor?.activeTabID
        return Binding(get: { currentDraft }, set: { value in
            guard model.core.snapshot?.editor.activeTabID == target else { return }
            draftTabID = target
            draft = value
            model.core.updateDraft(value)
            model.core.scheduleFileSave(value)
        })
    }

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
                    documentToolbar(selectedURL)
                    editorContent(for: selectedURL)
                    if let notice { noticeBar(systemImage: "exclamationmark.circle", message: notice, color: HideTheme.warning) }
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
            draftTabID = editor?.activeTabID
            notice = nil
        }
        .onChange(of: editor?.contentsUTF8) { _, contents in
            if let contents { draft = contents; draftTabID = editor?.activeTabID }
        }

    }

    @ViewBuilder
    private func editorContent(for url: URL) -> some View {
        if isImage(url) {
            imagePreview(url)
        } else if editor?.contentsUTF8 != nil {
            if isMarkdown && preview && currentDraft.isEmpty {
                unavailable(title: "Empty document", message: "Choose Edit to start writing Markdown.")
            } else if isMarkdown && preview {
                MarkdownPreview(text: currentDraft, textScale: model.editorTextScale, findRequest: findRequest, openLink: openDocumentLink)
                    .frame(maxWidth: HideTheme.Editor.documentWidth)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("markdown-preview")
            } else {
                HighlightedCodeEditor(
                    text: draftBinding,
                    language: editor?.language,
                    isEditable: readonlyReason == nil,
                    textScale: model.editorTextScale,
                    wrapsLines: wrapsLines,
                    findRequest: findRequest
                )
                .id(activeTab?.id)
            }
        } else {
            unavailable(
                title: "Preview only",
                message: editor?.readonlyReason ?? "This file type cannot be shown as text."
            )
        }
    }

    private func documentToolbar(_ url: URL) -> some View {
        HStack(spacing: HideTheme.spacingSM) {
            Text(breadcrumb(for: url))
                .hideFont(size: HideTheme.Typography.subhead)
                .foregroundStyle(HideTheme.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .hideTooltip(url.path)
                .frame(maxWidth: .infinity, alignment: .leading)
            if isMarkdown {
                HideChoiceGroup(label: "Markdown mode", values: [true, false],
                    selection: Binding(get: { preview }, set: { setView(preview: $0, wrap: wrapsLines) }),
                    title: { $0 ? "Preview" : "Edit" },
                    identifier: { $0 ? "markdown-mode-preview" : "markdown-mode-edit" },
                    optionHelp: { $0 ? "Read the current Markdown draft" : "Edit Markdown source" })
            }
            HStack(spacing: HideTheme.spacingXXS) {
                if editor?.dirty == true {
                    Text("Unsaved").hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.warning)
                }
                HideIconButton(systemImage: "magnifyingglass", help: "Find in document", variant: .toolbar, command: .menu(.findInPane)) { findRequest += 1 }
                    .disabled(editor?.contentsUTF8 == nil)
                if !(isMarkdown && preview) && !isImage(url) {
                    HideIconButton(systemImage: "arrow.turn.down.left", help: "Wrap lines", variant: .toolbar, isSelected: wrapsLines) {
                        setView(preview: preview, wrap: !wrapsLines)
                    }
                    .disabled(editor?.contentsUTF8 == nil)
                }
                HideIconButton(systemImage: "folder", help: "Reveal file in Explorer", variant: .toolbar) {
                    guard let tab = activeTab else { return }
                    model.core.revealPath(url, workspaceID: tab.workspaceID, checkoutID: tab.checkoutID, isDirectory: false)
                }
                HideIconButton(systemImage: "arrow.up.forward.square", help: "Reveal file in Finder", variant: .toolbar) {
                    NSWorkspace.shared.activateFileViewerSelecting([url])
                }
            }
            .frame(maxWidth: .infinity, alignment: .trailing)
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.vertical, HideTheme.spacingXS)
        .background(HideTheme.panel)
        .overlay(alignment: .bottom) { HideTheme.divider.frame(height: HideTheme.Layout.hairlineWidth) }
        .accessibilityIdentifier("file-document-toolbar")
    }

    private func breadcrumb(for url: URL) -> String {
        guard let tab = activeTab, let checkout = model.registeredCheckouts.first(where: { $0.id == tab.checkoutID }),
              url.path.hasPrefix(checkout.path + "/") else { return url.path }
        let relative = String(url.path.dropFirst(checkout.path.count + 1))
        return ([URL(fileURLWithPath: checkout.path).lastPathComponent] + relative.split(separator: "/").map(String.init)).joined(separator: " / ")
    }

    private func setView(preview: Bool, wrap: Bool) {
        guard let tab = activeTab else { return }
        model.core.setFileView(tabID: tab.id, preview: preview, wrap: wrap)
    }

    private func openDocumentLink(_ url: URL) {
        if ["https", "http"].contains(url.scheme?.lowercased() ?? "") {
            ExternalBrowser.open(url) { notice = $0 }
            return
        }
        guard url.scheme == nil || url.isFileURL, let selectedURL, let tab = activeTab,
              url.fragment == nil else {
            notice = "This link type is unavailable in Markdown preview."
            return
        }
        let target = url.isFileURL ? url : URL(fileURLWithPath: url.path, relativeTo: selectedURL.deletingLastPathComponent())
        let path = TerminalLinkResolver.canonical(target)
        guard let checkout = model.registeredCheckouts.first(where: { $0.id == tab.checkoutID }),
              path.path.hasPrefix(TerminalLinkResolver.canonical(URL(fileURLWithPath: checkout.path)).path + "/"),
              FileManager.default.fileExists(atPath: path.path) else {
            notice = "The linked file is missing or outside this checkout."
            return
        }
        model.core.revealPath(path, workspaceID: tab.workspaceID, checkoutID: tab.checkoutID, isDirectory: false)
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
