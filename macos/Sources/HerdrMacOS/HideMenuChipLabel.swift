import SwiftUI

/// Compact menu trigger appearance; the containing Menu owns activation.
struct HideMenuChipLabel: View {
    let title: String
    let image: Image
    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovered = false

    var body: some View {
        HStack(spacing: HideTheme.spacingXS) {
            image
                .hideFont(size: HideTheme.Typography.body, weight: .medium)
                .accessibilityHidden(true)
            Text(title).lineLimit(1)
            Image(systemName: "chevron.down")
                .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                .foregroundStyle(HideTheme.muted)
                .accessibilityHidden(true)
        }
        .hideFont(size: HideTheme.Typography.body, weight: .medium)
        .foregroundStyle(isHovered && isEnabled ? HideTheme.primary : HideTheme.secondary)
        .padding(.horizontal, HideTheme.spacingSM)
        .frame(minHeight: HideTheme.Control.compactHeight)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
        .overlay {
            RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                .allowsHitTesting(false)
        }
        .hideControlFocus(cornerRadius: HideTheme.radiusSmall)
        .opacity(isEnabled ? 1 : HideTheme.Opacity.disabled)
        .onHover { isHovered = $0 }
    }
}
