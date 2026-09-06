import SwiftUI

struct HideIconButton: View {
    let systemImage: String
    let help: String
    let accessibilityLabel: String
    var command: HideCommand? = nil
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                .foregroundStyle(HideTheme.secondary)
                .frame(width: 32, height: 32)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
        }
        .buttonStyle(.plain)
        .hideTooltip(help, command: command)
        .accessibilityLabel(accessibilityLabel)
    }
}


/// One icon control in a pane header.
///
/// The header has a single 28pt row to spend, so these are icon-only and carry
/// their meaning in a tooltip and an accessibility label rather than in text.
struct PaneHeaderButton: View {
    let systemImage: String
    let help: String
    let accessibilityLabel: String
    var command: HideCommand? = nil
    var paneID: String? = nil
    let action: () -> Void

    @State private var isHovering = false

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                .foregroundStyle(isHovering ? HideTheme.primary : HideTheme.secondary)
                .frame(
                    width: HideTheme.Layout.panelCollapseControlSize,
                    height: HideTheme.Layout.panelCollapseControlSize
                )
                .background(
                    RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                        .fill(isHovering ? HideTheme.elevated : Color.clear)
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovering = $0 }
        .hideTooltip(help, command: command, paneID: paneID)
        .accessibilityLabel(accessibilityLabel)
    }
}

