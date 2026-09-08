import Foundation

/// A semantic command is resolved when it is drawn, so a persisted rebind
/// cannot leave an old chord in keycaps, hover text, or accessibility help.
enum HideCommand: Hashable {
    case menu(ShellMenuCommand)
    case pane(PaneCommand)
    case tab(Int)
    case agent(Int)
    case label(String)

    func shortcut(bindings: [PaneCommand: PaneShortcut]) -> PaneShortcut? {
        switch self {
        case .menu(let command): command.shortcut
        case .pane(let command): bindings[command] ?? command.defaultShortcut
        case .tab(let number): (1...9).contains(number) ? PaneShortcut(key: String(number), modifiers: [.command]) : nil
        case .agent(let number): (1...9).contains(number) ? PaneShortcut(key: String(number), modifiers: [.option]) : nil
        case .label: nil
        }
    }

    func displayString(bindings: [PaneCommand: PaneShortcut]) -> String {
        if case .label(let label) = self { return label }
        guard let shortcut = shortcut(bindings: bindings) else { preconditionFailure("A keycap requires a numbered target from 1 through 9") }
        return shortcut.displayString
    }

    func tooltipText(label: String, bindings: [PaneCommand: PaneShortcut]) -> String {
        guard let shortcut = shortcut(bindings: bindings) else { return label }
        return "\(label) (\(shortcut.displayString))"
    }
}
