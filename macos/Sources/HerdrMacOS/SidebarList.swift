import SwiftUI

/// Let AppKit's table index rows for cursor/wheel hit testing, as the Explorer
/// does with its outline. A SwiftUI ScrollView walks the nested row responder
/// graph even when the system, rather than our window, asks for the target.
/// Row content keeps its own actions, selection, and styling.
struct SidebarList<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        List {
            Group {
                content
                Color.clear.frame(height: HideTheme.spacingLG).accessibilityHidden(true)
            }
            .listRowInsets(EdgeInsets())
            .listRowSeparator(.hidden)
            .listRowBackground(Color.clear)
        }
        .listStyle(.plain)
        .contentMargins(.all, HideTheme.spacingNone)
        .scrollContentBackground(.hidden)
    }
}
