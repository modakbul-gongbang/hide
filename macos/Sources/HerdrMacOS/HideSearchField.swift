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
        .hideInputSurface(focused: isFocused)
    }
}
