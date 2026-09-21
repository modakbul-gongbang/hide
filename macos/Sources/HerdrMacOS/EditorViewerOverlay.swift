import AppKit
import SwiftUI

struct EditorViewerOverlay: View {
    @EnvironmentObject private var model: ShellModel
    @State private var draft = ""
    @State private var draftTabID: String?
    @State private var pendingDraftEcho: String?
    @State private var findRequest = 0
    @State private var notice: String?

    private var documentKind: CoreDocumentKind? { editor?.documentKind }
    private var isMarkdown: Bool { documentKind == .markdown }
    /// Live is the tab's choice unless the document is too large for the Live
    /// view, which then reads Source and says so (D-07).
    private var live: Bool { (activeTab?.markdownLive ?? true) && !liveUnavailable }
    private var liveUnavailable: Bool { isMarkdown && currentDraft.utf8.count > MarkdownLiveSource.byteLimit }
    static let liveUnavailableNotice = "Live preview is off for files over 256 KB"
    private var wrapsLines: Bool { activeTab?.wrap ?? false }
    private var currentDraft: String { draftTabID == editor?.activeTabID ? draft : (editor?.contentsUTF8 ?? "") }
    private var draftBinding: Binding<String> {
        let target = editor?.activeTabID
        return Binding(get: { currentDraft }, set: { value in
            guard model.core.snapshot?.editor.activeTabID == target else { return }
            draftTabID = target
            draft = value
            pendingDraftEcho = value
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
            if let activeTab, activeTab.kind == .session || activeTab.kind == .memory,
               let detail = editor?.archiveDetail {
                ArchiveDetailView(detail: detail)
            } else if let activeTab, activeTab.kind == .diff {
                diffViewer(tab: activeTab)
            } else if let selectedURL {
                VStack(spacing: HideTheme.spacingNone) {
                    documentToolbar(selectedURL)
                    if liveUnavailable {
                        noticeBar(systemImage: "exclamationmark.circle", message: Self.liveUnavailableNotice, color: HideTheme.warning)
                            .accessibilityIdentifier("markdown-live-unavailable")
                    }
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
            if draftTabID != editor?.activeTabID {
                draft = editor?.contentsUTF8 ?? ""
                draftTabID = editor?.activeTabID
                pendingDraftEcho = nil
            }
            notice = nil
        }
        .onChange(of: editor?.contentsUTF8) { _, contents in
            guard let contents else { return }
            // The core can echo an earlier keystroke while AppKit already
            // holds the next one. Keep that presentation buffer until the
            // latest edit is acknowledged, without retaining an edit queue.
            if draftTabID == editor?.activeTabID,
               let pendingDraftEcho, contents != pendingDraftEcho { return }
            draft = contents
            draftTabID = editor?.activeTabID
            pendingDraftEcho = nil
        }

    }

    /// One view per document kind. The core decided the kind when it read the
    /// file; adding a kind is one case here and one variant there (D-01).
    @ViewBuilder
    private func editorContent(for url: URL) -> some View {
        switch documentKind {
        case .image:
            imagePreview(url)
        case .pdf:
            PDFDocumentSurface(url: url)
        case .binary:
            unavailable(title: "Preview only", message: "This file type cannot be shown as text.")
        case .text, .markdown, .none:
            textContent
        }
    }

    @ViewBuilder
    private var textContent: some View {
        if editor?.contentsUTF8 != nil {
            if isMarkdown && live {
                MarkdownLiveEditor(
                    text: draftBinding,
                    isEditable: readonlyReason == nil,
                    textScale: model.editorTextScale,
                    findRequest: findRequest,
                    openLink: openDocumentLink,
                    reportParseFailure: { notice = $0.map { "Live formatting is off: \($0)" } }
                )
                .id(activeTab?.id)
                .accessibilityIdentifier("markdown-live")
            } else {
                HighlightedCodeEditor(
                    text: draftBinding,
                    language: editor?.language,
                    isEditable: readonlyReason == nil,
                    textScale: model.editorTextScale,
                    wrapsLines: wrapsLines,
                    findRequest: findRequest,
                    markdownListEditing: isMarkdown
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
                    selection: Binding(get: { live }, set: { setView(live: $0, wrap: wrapsLines) }),
                    title: { $0 ? "Live" : "Source" },
                    identifier: { $0 ? "markdown-mode-live" : "markdown-mode-source" },
                    optionHelp: { $0 ? "Edit with formatting shown in place" : "Edit Markdown source" })
                    .disabled(liveUnavailable)
            }
            HStack(spacing: HideTheme.spacingXXS) {
                if editor?.dirty == true {
                    Text("Unsaved").hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.warning)
                }
                // PDFView has no find bar, so the control stays but says why it
                // is off; every other kind without text disables it as before.
                HideIconButton(
                    systemImage: "magnifyingglass",
                    help: documentKind == .pdf ? "Find is unavailable for PDF" : "Find in document",
                    variant: .toolbar,
                    command: documentKind == .pdf ? nil : .menu(.findInPane)
                ) { findRequest += 1 }
                    .disabled(editor?.contentsUTF8 == nil)
                if !(isMarkdown && live) && documentKind?.showsWrap == true {
                    HideIconButton(systemImage: "arrow.turn.down.left", help: "Wrap lines", variant: .toolbar, isSelected: wrapsLines) {
                        setView(live: live, wrap: !wrapsLines)
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

    private func setView(live: Bool, wrap: Bool) {
        guard let tab = activeTab else { return }
        model.core.setFileView(tabID: tab.id, live: live, wrap: wrap)
    }

    private func openDocumentLink(_ url: URL) {
        if ["https", "http"].contains(url.scheme?.lowercased() ?? "") {
            ExternalBrowser.open(url) { notice = $0 }
            return
        }
        guard url.scheme == nil || url.isFileURL, let selectedURL, let tab = activeTab,
              url.fragment == nil else {
            notice = "This link type cannot be opened from a Markdown document."
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
        // The image fills the document area like every other kind, so the
        // toolbar stays at the top instead of centring with a small image.
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func conflictBar(_ conflict: CoreEditorConflict) -> some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            Label("This file changed on disk. Your draft is preserved.", systemImage: "exclamationmark.triangle.fill")
            HStack {
                Button("Reload disk version") {
                    pendingDraftEcho = nil
                    model.core.resolveConflict("reload")
                }
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

}

private extension CoreDocumentKind {
    /// Wrap changes a text container. An image and a PDF have none and hide
    /// the control; a binary file keeps it disabled, as its preview-only
    /// state always has.
    var showsWrap: Bool {
        switch self {
        case .text, .markdown, .binary: true
        case .image, .pdf: false
        }
    }
}
