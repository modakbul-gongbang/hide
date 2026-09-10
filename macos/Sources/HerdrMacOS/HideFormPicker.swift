import SwiftUI

/// The native-shell menu-picker treatment for forms and sheets.
///
/// The visible label stays outside the native picker so the system does not
/// substitute its own labelled control chrome. The menu still supplies the
/// platform selection behaviour while Hide owns the surface hierarchy.
struct HideFormPicker<SelectionValue: Hashable, Content: View>: View {
    let label: String
    let selectedLabel: String
    /// False where the surrounding row already carries the label, such as a
    /// settings row whose label sits on the left. The accessibility label is
    /// still the picker's own, so the control never becomes anonymous.
    var showsFieldLabel = true
    /// A settings row gives the control the right-hand column rather than the
    /// whole width a form field takes.
    var width: CGFloat?
    @Binding var selection: SelectionValue
    @ViewBuilder let content: () -> Content

    init(
        _ label: String,
        selection: Binding<SelectionValue>,
        selectedLabel: String,
        showsFieldLabel: Bool = true,
        width: CGFloat? = nil,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.label = label
        self.selectedLabel = selectedLabel
        self.showsFieldLabel = showsFieldLabel
        self.width = width
        _selection = selection
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            if showsFieldLabel {
                fieldLabel
            }
            Menu {
                // In a settings row the visible label is the row's, so the
                // picker's own label would surface as a submenu title and put
                // every option one hover further away. Inline presentation is
                // what keeps a two-item choice one click deep; the stacked
                // form variant keeps the default, where the label is the
                // field's and the submenu never appears.
                let picker = Picker(label, selection: $selection, content: content)
                if showsFieldLabel {
                    picker
                } else {
                    picker.pickerStyle(.inline)
                }
            } label: {
                Text(selectedLabel).lineLimit(1)
            }
            .menuStyle(.borderlessButton)
            .frame(width: width, height: width == nil ? nil : HideTheme.settingsFieldHeight)
            .frame(maxWidth: width == nil ? .infinity : nil)
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
