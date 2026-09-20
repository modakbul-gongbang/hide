import SwiftUI

struct TerminalAttachmentNotice: View {
    @ObservedObject var bridge: CoreBridge
    let paneID: String

    var body: some View {
        if let operation = bridge.snapshot?.status.asyncOperations.last(where: { $0.kind == "terminal.attachment" && $0.targetID == paneID }) {
            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                Text(operation.message ?? "Preparing files…")
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(operation.phase == "pending" ? HideTheme.secondary : HideTheme.warning)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: HideTheme.spacingSM) {
                    if operation.retryable {
                        Button("Retry") { bridge.terminalAttachmentAction(requestID: operation.id, paneID: paneID, action: "retry") }
                            .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                    }
                    Button(operation.phase == "refused" ? "Dismiss" : "Cancel and discard held input") {
                        bridge.terminalAttachmentAction(requestID: operation.id, paneID: paneID, action: "cancel")
                    }
                    .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                    .disabled(operation.stage == "cancelling")
                }
            }
            .padding(.horizontal, HideTheme.spacingSM)
            .padding(.vertical, HideTheme.spacingXS)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(HideTheme.panel)
            .accessibilityIdentifier("terminal-attachment-\(paneID)")
        }
    }
}
