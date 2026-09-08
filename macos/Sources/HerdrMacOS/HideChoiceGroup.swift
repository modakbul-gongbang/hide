import SwiftUI

enum HideChoiceGroupAppearance {
    case tabs
    case segmented
}

/// A compact single-choice control with either tab or segmented presentation.
struct HideChoiceGroup<Value: Hashable>: View {
    let label: String
    let values: [Value]
    @Binding var selection: Value
    let title: (Value) -> String
    let appearance: HideChoiceGroupAppearance
    let identifier: (Value) -> String

    init(
        label: String,
        values: [Value],
        selection: Binding<Value>,
        title: @escaping (Value) -> String,
        appearance: HideChoiceGroupAppearance = .segmented,
        identifier: @escaping (Value) -> String = { _ in "" }
    ) {
        self.label = label
        self.values = values
        _selection = selection
        self.title = title
        self.appearance = appearance
        self.identifier = identifier
    }

    var body: some View {
        HStack(spacing: appearance == .tabs ? HideTheme.spacingLG : HideTheme.spacingXXS) {
            ForEach(values, id: \.self) { value in
                Button {
                    guard selection != value else { return }
                    selection = value
                } label: {
                    Text(title(value))
                }
                .buttonStyle(HideChoiceButtonStyle(
                    appearance: appearance,
                    isSelected: selection == value
                ))
                .accessibilityIdentifier(identifier(value))
                .accessibilityAddTraits(selection == value ? .isSelected : [])
            }
        }
        .padding(appearance == .segmented ? HideTheme.spacingXXS : HideTheme.spacingNone)
        .background {
            if appearance == .segmented {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .fill(HideTheme.sidebar)
            }
        }
        .overlay {
            if appearance == .segmented {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                    .allowsHitTesting(false)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(label)
    }
}

private struct HideChoiceButtonStyle: ButtonStyle {
    let appearance: HideChoiceGroupAppearance
    let isSelected: Bool

    func makeBody(configuration: Configuration) -> some View {
        HideChoiceButtonBody(
            label: configuration.label,
            isPressed: configuration.isPressed,
            appearance: appearance,
            isSelected: isSelected
        )
    }
}

private struct HideChoiceButtonBody<Label: View>: View {
    let label: Label
    let isPressed: Bool
    let appearance: HideChoiceGroupAppearance
    let isSelected: Bool

    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovered = false

    private var fontSize: CGFloat {
        appearance == .tabs ? HideTheme.Typography.subhead : HideTheme.Typography.body
    }

    private var foreground: Color {
        isSelected || isHovered ? HideTheme.primary : HideTheme.secondary
    }

    var body: some View {
        label
            .hideFont(size: fontSize, weight: .medium)
            .foregroundStyle(foreground)
            .padding(.horizontal, HideTheme.spacingSM)
            .frame(minHeight: HideTheme.Control.compactHeight)
            .background {
                if appearance == .segmented, isSelected {
                    RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        .fill(HideTheme.elevated)
                } else if appearance == .segmented, isHovered, isEnabled {
                    RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        .fill(HideTheme.primary.opacity(HideTheme.Opacity.subtleFill))
                }
            }
            .overlay(alignment: .bottom) {
                if appearance == .tabs, isSelected {
                    Rectangle()
                        .fill(HideTheme.primary)
                        .frame(height: HideTheme.Control.tabIndicatorHeight)
                        .allowsHitTesting(false)
                }
            }
            .contentShape(Rectangle())
            .hideControlFocus(cornerRadius: HideTheme.radiusSmall)
            .opacity(
                !isEnabled
                    ? HideTheme.Opacity.disabled
                    : (isPressed ? HideTheme.Opacity.secondary : 1)
            )
            .onHover { isHovered = $0 }
    }
}
