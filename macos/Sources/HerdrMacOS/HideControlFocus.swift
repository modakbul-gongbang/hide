import SwiftUI

/// Draws the shell's focus treatment from an existing control's focus state.
/// It never makes a view focusable or owns a `FocusState` binding.
private struct HideControlFocusModifier: ViewModifier {
    let cornerRadius: CGFloat
    @Environment(\.isFocused) private var isFocused

    func body(content: Content) -> some View {
        content
            .focusEffectDisabled()
            .overlay {
                if isFocused {
                    RoundedRectangle(cornerRadius: cornerRadius)
                        .stroke(
                            HideTheme.primary.opacity(HideTheme.Opacity.secondary),
                            lineWidth: HideTheme.Layout.hairlineWidth
                        )
                        .allowsHitTesting(false)
                }
            }
    }
}

extension View {
    func hideControlFocus(cornerRadius: CGFloat) -> some View {
        modifier(HideControlFocusModifier(cornerRadius: cornerRadius))
    }
}
