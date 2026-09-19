import SwiftUI

private struct HideAccentKey: EnvironmentKey {
    static let defaultValue = HideTheme.accent
}

private struct HidePetAppearanceKey: EnvironmentKey {
    static let defaultValue = false
}

private struct HideFontScaleKey: EnvironmentKey {
    static let defaultValue = CGFloat(1)
}

/// Whether the canvas a view sits on is the one on top. A retained canvas
/// for a tab that is not showing is kept in the tree for its scrollback,
/// and the terminal views on it read this to stop drawing while hidden.
private struct HideCanvasVisibleKey: EnvironmentKey {
    static let defaultValue = true
}

extension EnvironmentValues {
    var hidePetAppearance: Bool {
        get { self[HidePetAppearanceKey.self] }
        set { self[HidePetAppearanceKey.self] = newValue }
    }

    var hideAccent: Color {
        get { self[HideAccentKey.self] }
        set { self[HideAccentKey.self] = newValue }
    }

    var hideCanvasVisible: Bool {
        get { self[HideCanvasVisibleKey.self] }
        set { self[HideCanvasVisibleKey.self] = newValue }
    }

    var hideFontScale: CGFloat {
        get { self[HideFontScaleKey.self] }
        set { self[HideFontScaleKey.self] = newValue }
    }
}

private struct HideScaledFontModifier: ViewModifier {
    let size: CGFloat
    let weight: Font.Weight
    let design: Font.Design
    let italic: Bool
    @Environment(\.hideFontScale) private var scale
    @Environment(\.hidePetAppearance) private var petAppearance

    func body(content: Content) -> some View {
        content.font(petAppearance
            ? systemFont
            : HideTheme.font(size: size * scale, weight: weight, design: design, italic: italic))
    }

    private var systemFont: Font {
        let font = Font.system(size: size * scale, weight: weight, design: design)
        return italic ? font.italic() : font
    }
}

extension View {
    /// `italic` selects the theme's slanted variant (`HideTheme.Typography.previewSlant`);
    /// the strip draws a preview tab's title with it and nothing else does.
    func hideFont(
        size: CGFloat,
        weight: Font.Weight = .regular,
        design: Font.Design = .default,
        italic: Bool = false
    ) -> some View {
        modifier(HideScaledFontModifier(size: size, weight: weight, design: design, italic: italic))
    }
}
