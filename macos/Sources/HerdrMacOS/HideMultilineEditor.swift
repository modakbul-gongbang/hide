import SwiftUI

/// The shared multiline input owner. It keeps native editing semantics while
/// applying the same focused surface used by the shell's other text inputs.
struct HideMultilineEditor: View {
    @Binding var text: String
    let accessibilityLabel: String
    let minimumHeight: CGFloat
    @FocusState private var isFocused: Bool

    var body: some View {
        TextEditor(text: $text)
            .textEditorStyle(.plain)
            .focused($isFocused)
            .hideInputSurface(focused: isFocused)
            .frame(minHeight: minimumHeight)
            .accessibilityLabel(accessibilityLabel)
    }
}
