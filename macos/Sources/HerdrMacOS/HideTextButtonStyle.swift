import SwiftUI

/// A text action for native-shell forms, toolbars, and sheets.
///
/// Appearance describes visual emphasis while density owns the control's
/// typography and height. Button still owns activation, focus, role, and
/// disabled semantics.
struct HideTextButtonStyle: ButtonStyle {
    enum Appearance {
        case quiet
        case standard
        case prominent
    }

    enum Density {
        case compact
        case regular

        var height: CGFloat {
            switch self {
            case .compact: HideTheme.Control.compactHeight
            case .regular: HideTheme.Control.regularHeight
            }
        }

        var fontSize: CGFloat {
            switch self {
            case .compact: HideTheme.Typography.body
            case .regular: HideTheme.Typography.title
            }
        }

        var horizontalPadding: CGFloat {
            switch self {
            case .compact: HideTheme.spacingSM
            case .regular: HideTheme.spacingMD
            }
        }
    }

    let appearance: Appearance
    let density: Density

    init(
        appearance: Appearance = .standard,
        density: Density = .compact
    ) {
        self.appearance = appearance
        self.density = density
    }

    func makeBody(configuration: Configuration) -> some View {
        HideTextButtonBody(
            label: configuration.label,
            role: configuration.role,
            isPressed: configuration.isPressed,
            appearance: appearance,
            density: density
        )
    }
}

private struct HideTextButtonBody<Label: View>: View {
    let label: Label
    let role: ButtonRole?
    let isPressed: Bool
    let appearance: HideTextButtonStyle.Appearance
    let density: HideTextButtonStyle.Density

    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovered = false

    private var isDestructive: Bool { role == .destructive }

    private var foreground: Color {
        if isDestructive, appearance != .prominent { return HideTheme.danger }
        return appearance == .prominent ? HideTheme.background : HideTheme.primary
    }

    private var background: Color {
        switch appearance {
        case .quiet:
            return .clear
        case .standard:
            return HideTheme.elevated
        case .prominent:
            return isDestructive ? HideTheme.danger : HideTheme.accent
        }
    }

    private var hoverFill: Color {
        if appearance == .prominent {
            return HideTheme.primary.opacity(HideTheme.Opacity.subtleFill)
        }
        return (isDestructive ? HideTheme.danger : HideTheme.primary)
            .opacity(HideTheme.Opacity.subtleFill)
    }

    var body: some View {
        label
            .hideFont(size: density.fontSize, weight: .semibold)
            .foregroundStyle(foreground)
            .padding(.horizontal, density.horizontalPadding)
            .frame(minHeight: density.height)
            .background(background, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .overlay {
                if isHovered, isEnabled {
                    RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        .fill(hoverFill)
                        .allowsHitTesting(false)
                }
            }
            .overlay {
                if appearance == .standard {
                    RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                        .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
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
