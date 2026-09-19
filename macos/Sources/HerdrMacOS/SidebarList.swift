import SwiftUI

/// Let AppKit's table index rows for cursor/wheel hit testing, as the Explorer
/// does with its outline. A SwiftUI ScrollView walks the nested row responder
/// graph even when the system, rather than our window, asks for the target.
/// Row content keeps its own actions, selection, and styling.
struct SidebarList<Content: View>: View {
    /// A row the list scrolls to the top edge whenever this value becomes
    /// one. A row set that gains rows above its first row is otherwise left
    /// scrolled past them, because the list keeps the old first row where
    /// it was; the `Pinned` header appearing above the activity list is the
    /// case that showed it.
    var revealTopRowID: String? = nil
    @ViewBuilder let content: Content

    var body: some View {
        ScrollViewReader { proxy in
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
            .onChange(of: revealTopRowID, initial: true) { _, id in
                guard let id else { return }
                // The rows land in the list after this update, so the
                // scroll waits one turn of the run loop for them.
                DispatchQueue.main.async { proxy.scrollTo(id, anchor: .top) }
            }
        }
    }
}
