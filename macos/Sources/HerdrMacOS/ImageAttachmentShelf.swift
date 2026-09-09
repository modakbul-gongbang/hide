import AppKit
import SwiftUI

struct CoreAttachmentShelf: Decodable, Equatable {
    let paneID: String
    let items: [CoreImageAttachment]
    let notice: String?
    let followingBottom: Bool?
    let viewportMessage: String?
    enum CodingKeys: String, CodingKey { case paneID = "pane_id", items, notice
        case followingBottom = "following_bottom", viewportMessage = "viewport_message" }
}

struct CoreImageAttachment: Decodable, Equatable, Identifiable {
    let id: String
    let name: String
    let path: String?
    let state: String
    let message: String
    let provider: String?
}

struct ImageAttachmentShelf: View {
    @EnvironmentObject private var model: ShellModel
    let paneID: String
    @ObservedObject var viewport: TerminalViewportSignal
    private var shelf: CoreAttachmentShelf? { model.core.snapshot?.terminal.attachments?.first { $0.paneID == paneID } }

    var body: some View {
        if let shelf, shelf.items.contains(where: { $0.state != "dismissed" }) || shelf.notice != nil {
            if shelf.followingBottom != true || !viewport.followingBottom {
                collapsed(shelf)
            } else {
                VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                    ForEach(shelf.items.filter { $0.state != "dismissed" }) { item in
                        HStack(spacing: HideTheme.spacingSM) {
                            AttachmentThumbnail(path: item.path)
                            VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                                Text(item.name).hideFont(size: HideTheme.Typography.body, weight: .medium).lineLimit(1).truncationMode(.middle)
                                Text(item.message).hideFont(size: HideTheme.Typography.caption).foregroundStyle(item.state == "failed" ? HideTheme.warning : HideTheme.secondary)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            if item.state == "loading" || item.state == "queued" { ProgressView().controlSize(.small) }
                            if item.state == "ready" {
                                HideIconButton(systemImage: "plus.message", help: "Attach image to prompt; does not press Enter", variant: .toolbar) {
                                    // Recheck synchronous local state, including scroll events
                                    // whose coalesced view publication has not run yet.
                                    guard viewport.currentFollowingBottom else { return }
                                    model.focusPane(paneID)
                                    model.core.focusTerminal(paneID: paneID)
                                    model.core.attachmentAction("send", paneID: paneID, id: item.id)
                                }
                            }
                            HideIconButton(systemImage: "xmark", help: (item.state == "handoff_unconfirmed" || (item.state == "failed" && item.path != nil)) ? "Dismiss receipt; check the provider prompt separately" : "Remove attachment", variant: .toolbar) {
                                model.core.attachmentAction("remove", paneID: paneID, id: item.id)
                            }
                            .disabled(item.state == "queued")
                        }
                        .accessibilityIdentifier("image-attachment-\(item.state)")
                    }
                    if let notice = shelf.notice { Text(notice).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.warning) }
                }
                .padding(HideTheme.spacingSM)
                .background(HideTheme.panel)
                .overlay(alignment: .top) { HideTheme.divider.frame(height: HideTheme.Layout.hairlineWidth) }
                .accessibilityIdentifier("image-attachment-shelf")
            }
        }
    }
    private func collapsed(_ shelf: CoreAttachmentShelf) -> some View {
        let visible = shelf.items.filter { $0.state != "dismissed" }
        let pending = visible.filter { ["loading", "ready", "queued"].contains($0.state) }.count
        let failures = visible.filter { $0.state == "failed" }.count
        let receipts = visible.filter { $0.state == "handoff_unconfirmed" }.count
        let label = [pending > 0 ? "\(pending) images pending" : nil,
                     failures > 0 ? "\(failures) failed" : nil,
                     receipts > 0 ? "\(receipts) handoffs unconfirmed" : nil]
            .compactMap { $0 }.joined(separator: ", ")
        return HStack(spacing: HideTheme.spacingSM) {
            Button {
                guard viewport.returnToBottomAndFocus() else { return }
                model.focusPane(paneID)
                model.core.attachmentAction("return_to_prompt", paneID: paneID, id: "viewport")
            } label: {
                HStack(spacing: HideTheme.spacingSM) {
                    Image(systemName: "photo.on.rectangle")
                    Text(label).lineLimit(1)
                    Image(systemName: "arrow.down.to.line")
                    Text("Return to prompt").lineLimit(1)
                }
            }
            .buttonStyle(HideTextButtonStyle(appearance: .quiet))
            .hideTooltip("Return to prompt and focus terminal input")
            if let notice = shelf.notice {
                Text(notice).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.warning).lineLimit(1)
                    .hideTooltip(notice)
            } else if let failure = visible.first(where: { $0.state == "failed" }) {
                Text(failure.message).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.warning).lineLimit(1)
                    .hideTooltip(failure.message)
            } else if let message = shelf.viewportMessage, shelf.followingBottom == nil {
                Text(message).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.secondary).lineLimit(1)
                    .hideTooltip(message)
            }
            Spacer(minLength: HideTheme.spacingXS)
            HideIconButton(systemImage: "xmark", help: "Remove pending images and dismiss Hide receipts", variant: .toolbar) {
                for item in visible where item.state != "queued" {
                    model.core.attachmentAction("remove", paneID: paneID, id: item.id)
                }
            }
            .disabled(visible.allSatisfy { $0.state == "queued" })
        }
        .padding(.horizontal, HideTheme.spacingSM)
        .background(HideTheme.panel)
        .accessibilityIdentifier("image-attachment-return-to-prompt")
    }

}

private struct AttachmentThumbnail: View {
    let path: String?
    @State private var image: NSImage?
    var body: some View {
        Group {
            if let image { Image(nsImage: image).resizable().scaledToFit() }
            else { Image(systemName: "photo").foregroundStyle(HideTheme.secondary) }
        }
        .frame(width: HideTheme.Attachment.thumbnailSize, height: HideTheme.Attachment.thumbnailSize)
        .accessibilityHidden(true)
        .task(id: path) {
            image = path.flatMap { NSImage(contentsOf: URL(fileURLWithPath: $0).deletingLastPathComponent().appendingPathComponent("preview.png")) }
        }
    }
}

/// The feature's only canvas composition point. Both surfaces take layout space;
/// neither overlays raw transcript rows or pretends to know a composer rectangle.
struct AttachmentTerminalContent: View {
    @EnvironmentObject private var model: ShellModel
    @StateObject private var viewport = TerminalViewportSignal()
    let paneID: String
    let textScale: CGFloat
    let onFocus: @MainActor @Sendable () -> Void
    let onOpenLink: @MainActor @Sendable (String) -> Void

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            TerminalHost(bridge: model.core, paneID: paneID, textScale: textScale,
                onFocus: onFocus, onOpenLink: onOpenLink, viewportSignal: viewport)
                .accessibilityLabel("SwiftTerm terminal for \(paneID)")
            ImageAttachmentShelf(paneID: paneID, viewport: viewport)
        }
    }
}
