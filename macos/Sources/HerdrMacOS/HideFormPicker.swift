import SwiftUI

/// The native-shell menu-picker treatment for forms and sheets.
///
/// The visible label stays outside the native picker so the system does not
/// substitute its own labelled control chrome. The menu still supplies the
/// platform selection behaviour while Hide owns the surface hierarchy.
struct HideFormPicker<SelectionValue: Hashable, Content: View>: View {
    let label: String
    @Binding var selection: SelectionValue
    @ViewBuilder let content: () -> Content

    init(
        _ label: String,
        selection: Binding<SelectionValue>,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.label = label
        _selection = selection
        self.content = content
    }

    var body: some View {
        HStack(spacing: HideTheme.spacingMD) {
            Text(label)
                .hideFont(size: HideTheme.Typography.body)
                .foregroundStyle(HideTheme.secondary)
            Picker(label, selection: $selection, content: content)
                .pickerStyle(.menu)
                .labelsHidden()
                .tint(HideTheme.secondary)
                .hideFont(size: HideTheme.Typography.body)
                .foregroundStyle(HideTheme.primary)
                .padding(.horizontal, HideTheme.spacingSM)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(
                    HideTheme.elevated,
                    in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                )
                .overlay {
                    RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                }
        }
    }
}
