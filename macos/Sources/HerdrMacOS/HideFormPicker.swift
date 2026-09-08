import SwiftUI

/// The native-shell menu-picker treatment for forms and sheets.
///
/// The visible label stays outside the native picker so the system does not
/// substitute its own labelled control chrome. The menu still supplies the
/// platform selection behaviour while Hide owns the surface hierarchy.
struct HideFormPicker<SelectionValue: Hashable, Content: View>: View {
    let label: String
    var selectedLabel: String?
    @Binding var selection: SelectionValue
    @ViewBuilder let content: () -> Content

    init(
        _ label: String,
        selection: Binding<SelectionValue>,
        selectedLabel: String? = nil,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.label = label
        self.selectedLabel = selectedLabel
        _selection = selection
        self.content = content
    }

    var body: some View {
        if let selectedLabel {
            VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                fieldLabel
                Menu {
                    Picker(label, selection: $selection, content: content)
                } label: {
                    Text(selectedLabel).lineLimit(1)
                }
                .menuStyle(.borderlessButton)
                .hideFont(size: HideTheme.Typography.body)
                .foregroundStyle(HideTheme.primary)
                .padding(.horizontal, HideTheme.spacingMD)
                .frame(maxWidth: .infinity, minHeight: HideTheme.formControlHeight)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
                .overlay {
                    RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                }
                .accessibilityLabel(label)
                .accessibilityValue(selectedLabel)
            }
        } else {
            HStack(spacing: HideTheme.spacingMD) {
                fieldLabel
                Picker(label, selection: $selection, content: content)
                    .pickerStyle(.menu)
                    .labelsHidden()
                    .tint(HideTheme.secondary)
                    .hideFont(size: HideTheme.Typography.body)
                    .foregroundStyle(HideTheme.primary)
                    .padding(.horizontal, HideTheme.spacingSM)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
                    .overlay {
                        RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                            .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                    }
            }
        }
    }

    private var fieldLabel: some View {
        Text(label)
            .hideFont(size: HideTheme.Typography.body, weight: .medium)
            .foregroundStyle(HideTheme.secondary)
    }
}
