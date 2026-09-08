import SwiftUI

/// A text action for native-shell forms and sheets.
///
/// Prominent actions use the product accent; ordinary actions stay on the
/// neutral surface ladder. Disabled and pressed states use named opacity
/// tokens so the button never falls back to AppKit chrome.
struct HideTextButtonStyle: ButtonStyle {
    let isProminent: Bool
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .hideFont(size: HideTheme.Typography.body, weight: .semibold)
            .foregroundStyle(isProminent ? HideTheme.background : HideTheme.primary)
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)
            .background(
                isProminent ? HideTheme.accent : HideTheme.elevated,
                in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
            )
            .overlay {
                if !isProminent {
                    RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                }
            }
            .opacity(
                !isEnabled
                    ? HideTheme.Opacity.disabled
                    : (configuration.isPressed ? HideTheme.Opacity.secondary : 1)
            )
    }
}
