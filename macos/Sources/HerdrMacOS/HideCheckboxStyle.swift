import SwiftUI

/// Checkbox presentation that retains `Toggle` ownership and semantics.
struct HideCheckboxStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        Button {
            configuration.isOn.toggle()
        } label: {
            HStack(spacing: HideTheme.spacingSM) {
                Image(systemName: configuration.isOn ? "checkmark.square.fill" : "square")
                    .resizable()
                    .scaledToFit()
                    .frame(
                        width: HideTheme.Control.checkboxSize,
                        height: HideTheme.Control.checkboxSize
                    )
                    .foregroundStyle(configuration.isOn ? HideTheme.primary : HideTheme.secondary)

                configuration.label
                    .hideFont(size: HideTheme.Typography.subhead)
                    .foregroundStyle(HideTheme.primary)
            }
            .frame(minHeight: HideTheme.Control.compactHeight, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
    }
}
