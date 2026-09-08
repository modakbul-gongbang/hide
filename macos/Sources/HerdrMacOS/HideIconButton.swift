import SwiftUI

/// Icon-only actions choose a role; geometry and interaction appearance live here.
struct HideIconButton: View {
    enum Variant: CaseIterable {
        case standard
        case toolbar

        var size: CGSize {
            switch self {
            case .standard: HideTheme.IconButton.standardSize
            case .toolbar: HideTheme.IconButton.toolbarSize
            }
        }

        var fontSize: CGFloat {
            self == .standard ? HideTheme.Typography.body : HideTheme.Typography.caption
        }
    }

    let systemImage: String
    let help: String
    var accessibilityLabel: String? = nil
    var variant: Variant = .standard
    var isSelected = false
    var command: HideCommand? = nil
    var paneID: String? = nil
    var tabID: String? = nil
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .hideFont(size: variant.fontSize, weight: .semibold)
        }
        .buttonStyle(HideIconButtonStyle(variant: variant, isSelected: isSelected))
        .hideTooltip(help, command: command, paneID: paneID, tabID: tabID)
        .accessibilityLabel(accessibilityLabel ?? help)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

/// Hover is local to one control; it never publishes shell or runtime state.
private struct HideIconButtonStyle: ButtonStyle {
    let variant: HideIconButton.Variant
    let isSelected: Bool
    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovering = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(isSelected || (isEnabled && isHovering) ? HideTheme.primary : HideTheme.secondary)
            .frame(width: variant.size.width, height: variant.size.height)
            .background {
                RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    .fill(variant == .standard ? HideTheme.elevated : Color.clear)
                RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    .fill(HideTheme.primary.opacity(
                        isSelected ? HideTheme.Opacity.selectedFill
                            : (isEnabled && isHovering ? HideTheme.Opacity.subtleFill : 0)
                    ))
            }
            .contentShape(Rectangle())
            .opacity(!isEnabled ? HideTheme.Opacity.disabled
                : (configuration.isPressed ? HideTheme.Opacity.secondary : 1))
            .onHover { hovering in
                if isHovering != hovering { isHovering = hovering }
            }
    }
}
