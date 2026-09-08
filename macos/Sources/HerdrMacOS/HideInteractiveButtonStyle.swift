import SwiftUI

/// Interaction feedback for domain rows whose layout and typography stay local.
struct HideInteractiveButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        HideInteractiveButtonBody(
            label: configuration.label,
            isPressed: configuration.isPressed
        )
    }
}

private struct HideInteractiveButtonBody<Label: View>: View {
    let label: Label
    let isPressed: Bool

    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovered = false

    var body: some View {
        label
            .contentShape(Rectangle())
            .overlay {
                if isHovered, isEnabled {
                    RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        .fill(HideTheme.primary.opacity(HideTheme.Opacity.subtleFill))
                        .allowsHitTesting(false)
                }
            }
            .hideControlFocus(cornerRadius: HideTheme.radiusSmall)
            .opacity(
                !isEnabled
                    ? HideTheme.Opacity.disabled
                    : (isPressed ? HideTheme.Opacity.secondary : 1)
            )
            .onHover { isHovered = $0 }
    }
}
