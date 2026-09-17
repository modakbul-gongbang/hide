import SwiftUI

struct SheetHeader: View {
    let title: String
    let subtitle: String

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            Text(title)
                .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                .foregroundStyle(HideTheme.primary)
            Text(subtitle)
                .hideFont(size: HideTheme.Typography.body)
                .foregroundStyle(HideTheme.secondary)
        }
        .padding(.horizontal, HideTheme.spacingXL)
        .padding(.top, HideTheme.spacingXL)
        .padding(.bottom, HideTheme.spacingSM)
    }
}
