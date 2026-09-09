import SwiftUI

/// Icon-only actions choose a role; geometry and interaction appearance live here.
struct HideIconButton: View {
    enum Variant: CaseIterable {
        case standard
        case toolbar
        /// A persistent contrasting action over arbitrary image content.
        case imageOverlay

        var size: CGSize {
            switch self {
            case .standard, .imageOverlay: HideTheme.IconButton.standardSize
            case .toolbar: HideTheme.IconButton.toolbarSize
            }
        }

        var fontSize: CGFloat {
            self == .toolbar ? HideTheme.Typography.caption : HideTheme.Typography.body
        }
    }

    let image: Image
    var imageSize: CGFloat? = nil
    var color: Color? = nil
    let help: String
    var accessibilityLabel: String? = nil
    var variant: Variant = .standard
    var isSelected = false
    var command: HideCommand? = nil
    var paneID: String? = nil
    var tabID: String? = nil
    let action: () -> Void

    init(systemImage: String, help: String, accessibilityLabel: String? = nil,
         variant: Variant = .standard, isSelected: Bool = false, command: HideCommand? = nil,
         paneID: String? = nil, tabID: String? = nil, action: @escaping () -> Void) {
        self.image = Image(systemName: systemImage)
        self.help = help
        self.accessibilityLabel = accessibilityLabel
        self.variant = variant
        self.isSelected = isSelected
        self.command = command
        self.paneID = paneID
        self.tabID = tabID
        self.action = action
    }

    init(image: Image, imageSize: CGFloat, color: Color, help: String,
         variant: Variant = .toolbar, isSelected: Bool = false, action: @escaping () -> Void) {
        self.image = image
        self.imageSize = imageSize
        self.color = color
        self.help = help
        self.variant = variant
        self.isSelected = isSelected
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            if let imageSize {
                image.resizable().frame(width: imageSize, height: imageSize)
            } else {
                image.hideFont(size: variant.fontSize, weight: .semibold)
            }
        }
        .buttonStyle(HideIconButtonStyle(variant: variant, isSelected: isSelected, color: color))
        .hideTooltip(help, command: command, paneID: paneID, tabID: tabID)
        .accessibilityLabel(accessibilityLabel ?? help)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

/// Hover is local to one control; it never publishes shell or runtime state.
private struct HideIconButtonStyle: ButtonStyle {
    let variant: HideIconButton.Variant
    let isSelected: Bool
    let color: Color?
    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovering = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(color ?? (variant == .imageOverlay || isSelected || (isEnabled && isHovering) ? HideTheme.primary : HideTheme.secondary))
            .frame(width: variant.size.width, height: variant.size.height)
            .background {
                RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    .fill(variant == .imageOverlay ? HideTheme.background
                        : (variant == .standard ? HideTheme.elevated : Color.clear))
                RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    .fill(HideTheme.primary.opacity(
                        isSelected ? HideTheme.Opacity.selectedFill
                            : (isEnabled && isHovering ? HideTheme.Opacity.subtleFill : 0)
                    ))
                if variant == .imageOverlay {
                    RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                        .strokeBorder(HideTheme.secondary, lineWidth: HideTheme.Layout.hairlineWidth)
                }
            }
            .contentShape(Rectangle())
            .hideControlFocus(cornerRadius: HideTheme.radiusMedium)
            .opacity(!isEnabled ? HideTheme.Opacity.disabled
                : (configuration.isPressed ? HideTheme.Opacity.secondary : 1))
            .onHover { hovering in
                if isHovering != hovering { isHovering = hovering }
            }
    }
}
