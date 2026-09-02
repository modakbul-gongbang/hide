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
    /// The first descendant text view, used to put the file editor's own text
    /// view in front of the find action when focus is elsewhere in the window.
    func firstDescendantTextView() -> NSTextView? {
        if let textView = self as? NSTextView { return textView }
        for subview in subviews {
            if let found = subview.firstDescendantTextView() { return found }
        }
        return nil
    }
}
