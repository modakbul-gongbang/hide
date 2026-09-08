import SwiftUI

/// A themed query field using the existing single focus and IME keyboard owner.
struct HideSearchField: View {
    let placeholder: String
    @Binding var text: String
    @Binding var selection: HideSearchSelection
    let resultIDs: [String]
    let activate: () -> Void
    let dismiss: () -> Void
    @State private var isFocused = false
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(HideTheme.secondary).accessibilityHidden(true)
            TextField(placeholder, text: $text)
                .textFieldStyle(.plain)
                .accessibilityLabel(placeholder)
                .hideSearchKeyboard(selection: $selection, resultIDs: resultIDs,
                                    activate: activate, dismiss: dismiss,
                                    focusChanged: { isFocused = $0 })
            if !text.isEmpty {
                HideIconButton(systemImage: "xmark", help: "Clear search", variant: .toolbar) { text = "" }
            }
        }
        .hideFont(size: HideTheme.Typography.title)
        .foregroundStyle(HideTheme.primary)
        .padding(.horizontal, HideTheme.spacingSM)
        .frame(minHeight: HideTheme.Control.regularHeight)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
        .overlay {
            RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                .stroke(isFocused && isEnabled ? HideTheme.primary : HideTheme.divider,
                        lineWidth: HideTheme.Layout.hairlineWidth)
                .allowsHitTesting(false)
        }
        .opacity(isEnabled ? 1 : HideTheme.Opacity.disabled)
    }
}
