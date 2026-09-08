import SwiftUI

/// Appearance shared by native text inputs; focus and editing stay with the caller.
struct HideInputSurface: ViewModifier {
    var compact = false
    var focused = false
    var design: Font.Design = .default
    @Environment(\.isEnabled) private var isEnabled

    func body(content: Content) -> some View {
        content
            .hideFont(size: compact ? HideTheme.Typography.body : HideTheme.Typography.title, design: design)
            .foregroundStyle(HideTheme.primary)
            .padding(.horizontal, HideTheme.spacingSM)
            .frame(minHeight: compact ? HideTheme.Control.compactHeight : HideTheme.Control.regularHeight)
            .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .stroke(focused && isEnabled ? HideTheme.primary : HideTheme.divider,
                            lineWidth: HideTheme.Layout.hairlineWidth)
                    .allowsHitTesting(false)
            }
            .opacity(isEnabled ? 1 : HideTheme.Opacity.disabled)
    }
}

extension View {
    func hideInputSurface(compact: Bool = false, focused: Bool = false, design: Font.Design = .default) -> some View {
        modifier(HideInputSurface(compact: compact, focused: focused, design: design))
    }
}
