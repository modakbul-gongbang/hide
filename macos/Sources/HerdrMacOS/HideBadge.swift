import SwiftUI

struct HideBadge: View {
    let label: String
    let color: Color
    var dimmed = false

    var body: some View {
        Text(label)
            .hideFont(size: HideTheme.Typography.micro, weight: .medium)
            .foregroundStyle(dimmed ? color.opacity(HideTheme.Opacity.dimmed) : color)
            .padding(.horizontal, HideTheme.spacingXS)
            .frame(height: 16)
            .background(HideTheme.panel, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
    }
}
