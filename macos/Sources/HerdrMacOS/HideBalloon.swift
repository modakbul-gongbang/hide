import SwiftUI

/// One surface renderer for the hover label and modifier-held chip.
struct HideBalloon: View {
    enum Mode { case tooltip, hint }
    let command: HideCommand
    let label: String
    let mode: Mode
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        switch mode {
        case .tooltip:
            Text(command.tooltipText(label: label, bindings: model.paneShortcuts))
                .hideFont(size: HideTheme.Typography.subhead)
                .foregroundStyle(HideTheme.primary)
                .padding(.horizontal, HideTheme.spacingSM)
                .padding(.vertical, HideTheme.spacingXS)
                .frame(maxWidth: HideTheme.Hint.tooltipMaxWidth)
                .fixedSize(horizontal: false, vertical: true)
                .background(HideTheme.balloon, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
                .overlay {
                    RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                        .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                }
                .allowsHitTesting(false)
                .accessibilityHidden(true)
        case .hint:
            HideKeycap(command: command)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
        }
    }
}
