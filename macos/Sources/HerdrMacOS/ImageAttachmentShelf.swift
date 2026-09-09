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
    var removalError: String? = nil
    var handoffStarted: Bool = false
    enum CodingKeys: String, CodingKey {
        case id, name, path, state, message, provider
        case removalError = "removal_error"
        case handoffStarted = "handoff_started"
    }
}


struct ImageAttachmentShelf: View {
    @EnvironmentObject private var model: ShellModel
    let paneID: String
    @ObservedObject var viewport: TerminalViewportSignal
    private var shelf: CoreAttachmentShelf? { model.core.snapshot?.terminal.attachments?.first { $0.paneID == paneID } }

    var body: some View {
        if let shelf, !shelf.items.isEmpty || shelf.notice != nil {
            if shelf.followingBottom != true || !viewport.followingBottom {
                collapsed(shelf)
            } else {
                VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                    ImageAttachmentGrid(items: shelf.items) { item in
                        model.core.attachmentAction("remove", paneID: paneID, id: item.id)
                    }
                    if let removal = shelf.items.first(where: { $0.removalError != nil }), let reason = removal.removalError {
                        Text("\(removal.name): Remove in the provider prompt. Hide cannot confirm removal.").hideFont(size: HideTheme.Typography.caption)
                            .foregroundStyle(HideTheme.warning).fixedSize(horizontal: false, vertical: true)
                            .hideTooltip(reason)
                    } else if let failure = shelf.items.first(where: { $0.state == "failed" }) {
                        Text("\(failure.name): \(failure.message)").hideFont(size: HideTheme.Typography.caption)
                            .foregroundStyle(HideTheme.warning).fixedSize(horizontal: false, vertical: true)
                    } else {
                        Text(shelf.items.contains { !$0.handoffStarted }
                            ? "Not sent. X removes an image. Enter adds the remaining images to the prompt."
                            : "Check the provider's image indicators, then press Enter to send. Images already handed off must be edited in the provider prompt.")
                            .hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.secondary)
                            .fixedSize(horizontal: false, vertical: true)
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
        let visible = shelf.items
        let pending = visible.filter { ["loading", "pending", "queued"].contains($0.state) }.count
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
            } else if let reason = visible.compactMap(\.removalError).first {
                Text(reason).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.warning).lineLimit(1)
                    .hideTooltip(reason)
            } else if let failure = visible.first(where: { $0.state == "failed" }) {
                Text(failure.message).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.warning).lineLimit(1)
                    .hideTooltip(failure.message)
            } else if let message = shelf.viewportMessage, shelf.followingBottom == nil {
                Text(message).hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.secondary).lineLimit(1)
                    .hideTooltip(message)
            }
            Spacer(minLength: HideTheme.spacingXS)

        }
        .padding(.horizontal, HideTheme.spacingSM)
        .background(HideTheme.panel)
        .accessibilityIdentifier("image-attachment-return-to-prompt")
    }

}

/// A bounded adaptive grid uses actual available width; every preview stays square.
/// The remove control addresses the stable intent, never a terminal cell/index.
struct ImageAttachmentGrid: View {
    let items: [CoreImageAttachment]
    let remove: (CoreImageAttachment) -> Void

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: HideTheme.Attachment.thumbnailSize,
                                              maximum: HideTheme.Attachment.thumbnailSize), spacing: HideTheme.spacingSM)],
                  alignment: .leading, spacing: HideTheme.spacingSM) {
            ForEach(items) { item in
                ZStack(alignment: .topTrailing) {
                    AttachmentThumbnail(path: item.path)
                    if !item.handoffStarted {
                        HideIconButton(systemImage: "xmark", help: "Remove \(item.name) before sending", variant: .imageOverlay) {
                            remove(item)
                        }
                    }
                }
                    .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
                    .overlay(alignment: .bottomLeading) {
                        if item.state == "loading" || item.state == "queued" {
                            ProgressView().controlSize(.small).padding(HideTheme.spacingXXS)
                        } else {
                            Image(systemName: item.state == "failed" || item.removalError != nil ? "exclamationmark.triangle" : (item.handoffStarted ? "clock" : "photo"))
                                .foregroundStyle(item.state == "failed" || item.removalError != nil ? HideTheme.warning : HideTheme.secondary)
                                .padding(HideTheme.spacingXXS).accessibilityHidden(true)
                        }
                    }
                    .hideTooltip("\(item.name): \(item.removalError ?? item.message)")
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel("\(item.name): \(item.removalError ?? item.message)")
                    .accessibilityIdentifier("image-attachment-\(item.state)")
            }
        }
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

    private var availability: Bool {
        model.core.snapshot?.focusedPaneID == paneID
    }
    private var hasShelf: Bool {
        model.core.snapshot?.terminal.attachments?.contains { $0.paneID == paneID } == true
    }
    private func publishAvailability(_ available: Bool) {
        guard hasShelf else { return }
        model.core.attachmentAction("viewport", paneID: paneID, id: "viewport",
            followingBottom: viewport.currentFollowingBottom, active: available)
    }

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            TerminalHost(bridge: model.core, paneID: paneID, textScale: textScale,
                onFocus: onFocus, onOpenLink: onOpenLink, viewportSignal: viewport)
                .accessibilityLabel("SwiftTerm terminal for \(paneID)")
            ImageAttachmentShelf(paneID: paneID, viewport: viewport)
        }
        .onAppear { publishAvailability(availability) }
        .onChange(of: availability) { publishAvailability($0) }
        .onChange(of: viewport.followingBottom) { _ in publishAvailability(availability) }
        .onChange(of: hasShelf) { _ in publishAvailability(availability) }
        .onDisappear { publishAvailability(false) }
    }
}
