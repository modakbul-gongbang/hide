import SwiftUI
import AppKit

struct BrowserPaneView: View {
    let pane: CorePaneSnapshot
    let binding: BrowserPaneBinding
    let isFocused: Bool
    let isZoomed: Bool
    let onFocus: () -> Void
    let onClose: () -> Void
    @Environment(\.hideCanvasVisible) private var visible
    @StateObject private var controller = BrowserPaneController()
    @State private var addressDraft = ""
    @State private var reconnect = 0
    @FocusState private var editingAddress: Bool

    private struct ConnectionKey: Hashable {
        let binding: BrowserPaneBinding
        let visible: Bool
        let reconnect: Int
    }

    var body: some View {
        HideTerminalPaneCard(
            paneID: pane.id,
            kind: "browser",
            closeHelp: binding.closeConsequence,
            title: "\(pane.herdrLabel ?? "Browser") · \(binding.profile)",
            status: controller.notice == nil ? "ready" : "unavailable",
            statusMessage: controller.notice,
            isFocused: isFocused,
            isZoomed: isZoomed,
            onFocus: onFocus,
            onReconnect: { reconnect += 1 },
            onClose: onClose
        ) {
            VStack(spacing: HideTheme.spacingNone) {
                HStack(spacing: HideTheme.spacingSM) {
                    HideIconButton(
                        systemImage: "arrow.clockwise", help: "Reload this page",
                        accessibilityLabel: "Reload browser pane \(pane.id)",
                        variant: .toolbar
                    ) { controller.send("Page.reload") }
                    .disabled(!controller.connected)
                    TextField("Page address", text: $addressDraft)
                        .textFieldStyle(.plain)
                        .hideFont(size: HideTheme.Typography.subhead)
                        .focused($editingAddress)
                        .onSubmit { controller.navigate(addressDraft) }
                        .accessibilityIdentifier("browser-address-\(pane.id)")
                    HideIconButton(
                        systemImage: "doc.on.doc", help: "Copy the CDP endpoint for this browser",
                        accessibilityLabel: "Copy CDP endpoint for pane \(pane.id)",
                        variant: .toolbar
                    ) {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString("http://127.0.0.1:\(binding.cdpPort)", forType: .string)
                    }
                }
                .padding(.horizontal, HideTheme.spacingSM)
                .frame(height: HideTheme.compactControlSize)
                .foregroundStyle(HideTheme.primary)
                .background(HideTheme.elevated)
                ZStack {
                    BrowserCanvas(controller: controller, paneID: pane.id, onFocus: onFocus)
                    if controller.image == nil {
                        VStack(spacing: HideTheme.spacingSM) {
                            if controller.notice == nil { ProgressView().controlSize(.small) }
                            Text(controller.notice == nil ? "Connecting to \(binding.profile)…" : "Browser disconnected")
                                .hideFont(size: HideTheme.Typography.subhead)
                                .foregroundStyle(HideTheme.secondary)
                        }
                        .allowsHitTesting(false)
                    }
                }
            }
        }
        .task(id: ConnectionKey(binding: binding, visible: visible, reconnect: reconnect)) {
            if visible { await controller.run(binding: binding) }
        }
        .onChange(of: controller.address) { _, value in
            if !editingAddress { addressDraft = value }
        }
    }
}
