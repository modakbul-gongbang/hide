import AppKit

/// Which surface `⌘F` searches.
///
/// Both surfaces already carry a find bar - SwiftTerm's for the terminal, and
/// AppKit's own for the code editor - so this decides which one to reveal
/// rather than introducing a third search UI.
enum PaneFindTarget: Equatable {
    /// The file editor, which overlays the terminal surface whenever a file
    /// tab is open, so an open tab means the editor is what is on screen.
    case fileEditor
    case terminal(paneID: String)
    /// Nothing is on screen to search. The chord does nothing rather than
    /// revealing a bar over an empty surface.
    case none
}

enum PaneFindPolicy {
    /// The same rule the zoom chords use for their target, for the same
    /// reason: the chord acts on what the operator is looking at.
    static func target(
        activeFileTabID: String?,
        focusedPaneID: String?,
        isRemoteContext: Bool
    ) -> PaneFindTarget {
        if !isRemoteContext, activeFileTabID != nil {
            return .fileEditor
        }
        guard let focusedPaneID, !focusedPaneID.isEmpty else { return .none }
        return .terminal(paneID: focusedPaneID)
    }
}

/// The counter shown for a search over a pane's whole scrollback.
///
/// Separate from SwiftTerm's own summary because it says something that one
/// cannot: Herdr caps the history it returns, so a capped count is marked
/// rather than presented as the total. A number that silently means "of what
/// we happened to look at" is the defect this whole path exists to fix.
enum PaneFindSummary {
    static func text(index: Int, total: Int, truncated: Bool) -> String {
        if total == 0 {
            return "No matches"
        }
        let count = truncated ? "\(total)+" : "\(total)"
        if index == 0 {
            return "\(count) matches"
        }
        return "\(index)/\(count)"
    }
}

/// Sends AppKit's own find action down the responder chain.
///
/// Both find bars are reached through `performTextFinderAction(_:)`, and both
/// read the action out of the sender's `tag`, so the sender is a menu item
/// carrying the tag rather than the live menu.
enum FindResponderAction {
    @MainActor
    static func send(_ action: NSTextFinder.Action) {
        let item = NSMenuItem()
        item.tag = action.rawValue
        NSApp.sendAction(
            #selector(NSResponder.performTextFinderAction(_:)),
            to: nil,
            from: item
        )
    }
}

extension NSView {
    /// The first descendant text view that carries a find bar, used to put the
    /// file editor's own text view in front of the find action when focus is
    /// elsewhere in the window.
    ///
    /// The window holds other text views - the terminal's marked-text overlay,
    /// and whatever field editor is active - so the search is narrowed to the
    /// one property that is only ever set on the editor. Matching any text view
    /// would hand the find action to a surface that has no bar to show.
    func firstDescendantFindableTextView() -> NSTextView? {
        if let textView = self as? NSTextView, textView.usesFindBar { return textView }
        for subview in subviews {
            if let found = subview.firstDescendantFindableTextView() { return found }
        }
        return nil
    }
}
