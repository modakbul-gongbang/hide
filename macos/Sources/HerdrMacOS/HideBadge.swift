import SwiftUI

struct HideBadge: View {
    let label: String
    let color: Color
    var dimmed = false
    var leadingImage: Image? = nil

    var body: some View {
        HStack(spacing: HideTheme.spacingXXS) {
            if let leadingImage {
                leadingImage
                    .resizable()
                    .scaledToFit()
                    .frame(width: HideTheme.Typography.micro, height: HideTheme.Typography.micro)
            }
            Text(label)
                .hideFont(size: HideTheme.Typography.micro, weight: .medium)
        }
            .foregroundStyle(dimmed ? color.opacity(HideTheme.Opacity.dimmed) : color)
            .padding(.horizontal, HideTheme.spacingXS)
            .frame(height: HideTheme.badgeHeight)
            .background(HideTheme.panel, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
    }
}
