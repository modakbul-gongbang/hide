import SwiftUI

struct HideBadge: View {
    let label: String
    let color: Color
    var dimmed = false
    var leadingImage: Image? = nil
    /// A bounded identity badge keeps one line and preserves both ends of a name.
    var maximumWidth: CGFloat? = nil

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
                .fixedSize(horizontal: maximumWidth == nil, vertical: false)
        }
            .foregroundStyle(dimmed ? color.opacity(HideTheme.Opacity.dimmed) : color)
            .lineLimit(maximumWidth == nil ? nil : 1)
            .truncationMode(maximumWidth == nil ? .tail : .middle)
            .padding(.horizontal, HideTheme.spacingXS)
            .frame(height: HideTheme.badgeHeight)
            .frame(maxWidth: maximumWidth)
            .background(HideTheme.panel, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
    }
}
