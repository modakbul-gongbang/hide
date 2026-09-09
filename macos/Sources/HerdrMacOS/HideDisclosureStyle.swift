import SwiftUI

/// Disclosure presentation that keeps `DisclosureGroup` as the state owner.
struct HideDisclosureStyle: DisclosureGroupStyle {
    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            Button {
                configuration.isExpanded.toggle()
            } label: {
                HStack(spacing: HideTheme.spacingXS) {
                    Image(systemName: "chevron.right")
                        .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                        .foregroundStyle(HideTheme.secondary)
                        .frame(width: HideTheme.spacingLG)
                        .rotationEffect(configuration.isExpanded ? .degrees(90) : .zero)

                    configuration.label
                        .hideFont(size: HideTheme.Typography.subhead, weight: .medium)
                        .foregroundStyle(HideTheme.primary)
                }
                .frame(minHeight: HideTheme.Control.compactHeight)
                .contentShape(Rectangle())
            }
            .buttonStyle(HideInteractiveButtonStyle())

            if configuration.isExpanded {
                configuration.content
                    .padding(.top, HideTheme.spacingSM)
            }
        }
    }
}
