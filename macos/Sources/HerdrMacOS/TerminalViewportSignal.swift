import AppKit
import Combine
import SwiftTerm

/// Read-only local viewport observation. No provider or attachment knowledge.
/// Herdr's host viewport is a separate authoritative signal: a frame renderer
/// can be locally at bottom while displaying Herdr scrollback.
@MainActor
final class TerminalViewportSignal: ObservableObject {
    @Published private(set) var followingBottom = false
    private(set) var currentFollowingBottom = false
    private weak var terminal: TerminalView?
    private var publicationPending = false

    func observe(_ terminal: TerminalView) {
        self.terminal = terminal
        currentFollowingBottom = !terminal.canScroll || terminal.scrollPosition >= 1
        guard followingBottom != currentFollowingBottom, !publicationPending else { return }
        publicationPending = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.publicationPending = false
            if self.followingBottom != self.currentFollowingBottom {
                self.followingBottom = self.currentFollowingBottom
            }
        }
    }

    @discardableResult func returnToBottomAndFocus() -> Bool {
        guard let terminal, let window = terminal.window else { return false }
        terminal.scroll(toPosition: 1)
        observe(terminal)
        return window.makeFirstResponder(terminal)
    }
}
