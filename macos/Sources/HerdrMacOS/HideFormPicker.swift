import SwiftUI

/// The native-shell menu-picker treatment for forms and sheets.
///
/// The visible label stays outside the native picker so the system does not
/// substitute its own labelled control chrome. The menu still supplies the
/// platform selection behaviour while Hide owns the surface hierarchy.
struct HideFormPicker<SelectionValue: Hashable, Content: View>: View {
    let label: String
    let selectedLabel: String
    @Binding var selection: SelectionValue
    @ViewBuilder let content: () -> Content

    init(
        _ label: String,
        selection: Binding<SelectionValue>,
        selectedLabel: String,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.label = label
        self.selectedLabel = selectedLabel
        _selection = selection
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            fieldLabel
            Menu {
                Picker(label, selection: $selection, content: content)
            } label: {
                Text(selectedLabel).lineLimit(1)
            }
            .menuStyle(.borderlessButton)
            .frame(maxWidth: .infinity)
            .hideInputSurface()
            .hideControlFocus(cornerRadius: HideTheme.radiusSmall)
            .accessibilityLabel(label)
            .accessibilityValue(selectedLabel)
        }
    }

    private var fieldLabel: some View {
        Text(label)
            .hideFont(size: HideTheme.Typography.body, weight: .medium)
            .foregroundStyle(HideTheme.secondary)
    }
}
