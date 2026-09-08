import AppKit
import Foundation

/// Time is supplied by the caller, so release/replace races can be tested
/// without a wall clock. One hold owns one deadline and one modifier set.
struct HideHintState: Equatable {
    private(set) var modifiers: Set<PaneShortcut.Modifier> = []
    private(set) var deadline: TimeInterval?
    private(set) var revealed = false

    mutating func update(_ modifiers: Set<PaneShortcut.Modifier>, at now: TimeInterval) {
        guard modifiers != self.modifiers else { return }
        let wasRevealed = revealed
        self.modifiers = modifiers
        revealed = !modifiers.isEmpty && wasRevealed
        deadline = modifiers.isEmpty || revealed ? nil : now + HideTheme.Hint.delay
    }

    mutating func advance(to now: TimeInterval) {
        guard let deadline, now >= deadline else { return }
        revealed = !modifiers.isEmpty
        self.deadline = nil
    }

    mutating func clear() { self = Self() }

    func reveals(_ command: HideCommand, bindings: [PaneCommand: PaneShortcut]) -> Bool {
        revealed && command.shortcut(bindings: bindings)?.modifiers == modifiers
    }

    static func modifiers(in flags: NSEvent.ModifierFlags) -> Set<PaneShortcut.Modifier> {
        var result: Set<PaneShortcut.Modifier> = []
        if flags.contains(.command) { result.insert(.command) }
        if flags.contains(.control) { result.insert(.control) }
        if flags.contains(.option) { result.insert(.option) }
        if flags.contains(.shift) { result.insert(.shift) }
        return result
    }
}

/// Only controls that exist in the current view tree are candidates.
/// Snapshot ownership is evaluated on each read, never cached with the hold.
struct HideHintTarget: Hashable {
    let id: String
    let command: HideCommand
    var paneID: String? = nil
    var tabID: String? = nil

    static func exposed(
        among targets: [Self], state: HideHintState,
        bindings: [PaneCommand: PaneShortcut], focusedPaneID: String?, activeTabID: String?
    ) -> Set<Self> {
        guard state.revealed else { return [] }
        return Set(targets.filter { target in
            switch target.command {
            case .menu(let command):
                guard [.search, .newChat, .newWorkspace, .toggleSidebarView,
                       .toggleLeftSidebar, .closeTab, .newTab, .toggleRightPanel].contains(command)
                else { return false }
                if command == .closeTab {
                    guard let tabID = target.tabID, tabID == activeTabID else { return false }
                }
            case .pane(let command):
                guard command == .closePane || command == .toggleZoom,
                      let paneID = target.paneID, paneID == focusedPaneID else { return false }
            case .tab, .agent: break
            case .label: return false
            }
            return state.reveals(target.command, bindings: bindings)
        })
    }
}

/// The observer returns the same event. Existing shortcut routing remains
/// downstream and retains sole authority to consume a key press.
@MainActor
enum HideHintEventObserver {
    static func observe(_ event: NSEvent, update: (Set<PaneShortcut.Modifier>) -> Void) -> NSEvent {
        if event.type == .flagsChanged { update(HideHintState.modifiers(in: event.modifierFlags)) }
        return event
    }
}
