import AppKit
import SwiftTerm
import SwiftUI

enum PaneCommand: String, CaseIterable, Hashable, Identifiable, Sendable {
    case splitRight = "split_right"
    case splitDown = "split_down"
    case toggleZoom = "toggle_zoom"
    case closePane = "close_pane"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .splitRight: "Split Right"
        case .splitDown: "Split Down"
        case .toggleZoom: "Toggle Zoom"
        case .closePane: "Close Pane"
        }
    }

    var defaultShortcut: PaneShortcut {
        switch self {
        case .splitRight: PaneShortcut(key: "d", modifiers: [.command])
        case .splitDown: PaneShortcut(key: "d", modifiers: [.command, .shift])
        case .toggleZoom: PaneShortcut(key: "return", modifiers: [.command, .option])
        case .closePane: PaneShortcut(key: "w", modifiers: [.command, .shift])
        }
    }
}

struct PaneShortcut: Equatable, Hashable, Sendable {
    enum Modifier: String, CaseIterable, Hashable, Sendable {
        case command
        case control
        case option
        case shift
    }

    let key: String
    let modifiers: Set<Modifier>

    var canonical: String {
        let ordered = Modifier.allCases.filter(modifiers.contains).map(\.rawValue)
        return (ordered + [key]).joined(separator: "+")
    }

    /// The chord as the operator reads it on a menu or in help text. Written
    /// once here so a rebind cannot leave a stale chord printed in a tooltip,
    /// which is how a retired chord outlived the binding it described.
    var displayString: String {
        let symbols: [Modifier: String] = [
            .control: "⌃",
            .option: "⌥",
            .shift: "⇧",
            .command: "⌘",
        ]
        // macOS prints modifiers in a fixed order regardless of how they were
        // declared: control, option, shift, command.
        let order: [Modifier] = [.control, .option, .shift, .command]
        let prefix = order.filter(modifiers.contains).compactMap { symbols[$0] }.joined()
        return prefix + (key == "return" ? "↩" : key.uppercased())
    }

    var keyEquivalent: KeyEquivalent {
        key == "return" ? .return : KeyEquivalent(Character(key))
    }

    var eventModifiers: EventModifiers {
        var result: EventModifiers = []
        if modifiers.contains(.command) { result.insert(.command) }
        if modifiers.contains(.control) { result.insert(.control) }
        if modifiers.contains(.option) { result.insert(.option) }
        if modifiers.contains(.shift) { result.insert(.shift) }
        return result
    }

    func matches(_ event: NSEvent) -> Bool {
        guard event.type == .keyDown else { return false }
        let relevantFlags = event.modifierFlags.intersection([
            .command, .control, .option, .shift,
        ])
        var expectedFlags: NSEvent.ModifierFlags = []
        if modifiers.contains(.command) { expectedFlags.insert(.command) }
        if modifiers.contains(.control) { expectedFlags.insert(.control) }
        if modifiers.contains(.option) { expectedFlags.insert(.option) }
        if modifiers.contains(.shift) { expectedFlags.insert(.shift) }
        guard relevantFlags == expectedFlags else { return false }
        if key == "return" {
            return event.keyCode == 36 || event.keyCode == 76
        }
        return event.charactersIgnoringModifiers?.lowercased() == key
    }

    static func parse(_ raw: String) throws -> PaneShortcut {
        let components = raw
            .lowercased()
            .replacingOccurrences(of: " ", with: "")
            .split(separator: "+", omittingEmptySubsequences: false)
            .map(String.init)
        guard components.count >= 2, let rawKey = components.last, !rawKey.isEmpty else {
            throw PaneShortcutValidationError.invalidFormat
        }

        var modifiers = Set<Modifier>()
        for component in components.dropLast() {
            let modifier: Modifier? = switch component {
            case "cmd", "command": .command
            case "ctrl", "control": .control
            case "opt", "option", "alt": .option
            case "shift": .shift
            default: nil
            }
            guard let modifier, modifiers.insert(modifier).inserted else {
                throw PaneShortcutValidationError.invalidFormat
            }
        }
        guard modifiers.contains(.command) else {
            throw PaneShortcutValidationError.commandRequired
        }

        let key = switch rawKey {
        case "enter", "return": "return"
        default: rawKey
        }
        guard key == "return"
            || (key.count == 1 && key.unicodeScalars.allSatisfy({ $0.isASCII && !$0.properties.isWhitespace }))
        else {
            throw PaneShortcutValidationError.unsupportedKey
        }
        return PaneShortcut(key: key, modifiers: modifiers)
    }
}

enum PaneShortcutValidationError: Error, Equatable, LocalizedError {
    case invalidFormat
    case commandRequired
    case unsupportedKey
    case reserved(String)
    case duplicate(String)

    var errorDescription: String? {
        switch self {
        case .invalidFormat:
            "Use a chord such as command+d or command+option+return."
        case .commandRequired:
            "Pane shortcuts must include Command so terminal typing remains available."
        case .unsupportedKey:
            "Use one printable key or Return."
        case let .reserved(binding):
            "\(binding) is reserved by an existing application command."
        case let .duplicate(command):
            "This shortcut is already assigned to \(command)."
        }
    }
}

struct PaneShortcutResolution: Equatable, Sendable {
    let bindings: [PaneCommand: PaneShortcut]
    let diagnostic: String?
}

enum PaneShortcutPolicy {
    private static let reserved = Set([
        "command+,",
        "command+q",
        "command+h",
        "command+m",
        "command+1",
        "command+2",
        "command+3",
        "command+s",
        "command+w",
    ])

    static var defaults: [PaneCommand: PaneShortcut] {
        Dictionary(uniqueKeysWithValues: PaneCommand.allCases.map { ($0, $0.defaultShortcut) })
    }

    static func resolve(stored: [String: String]) -> PaneShortcutResolution {
        guard !stored.isEmpty else {
            return PaneShortcutResolution(bindings: defaults, diagnostic: nil)
        }
        let knownKeys = Set(PaneCommand.allCases.map(\.rawValue))
        let unknownKeys = Set(stored.keys).subtracting(knownKeys)
        guard unknownKeys.isEmpty else {
            return fallback("Unknown pane shortcut keys were ignored: \(unknownKeys.sorted().joined(separator: ", ")).")
        }

        var resolved = defaults
        do {
            for command in PaneCommand.allCases {
                guard let raw = stored[command.rawValue] else { continue }
                let shortcut = try PaneShortcut.parse(raw)
                if reserved.contains(shortcut.canonical) {
                    throw PaneShortcutValidationError.reserved(shortcut.canonical)
                }
                resolved[command] = shortcut
            }
            let collisions = Dictionary(grouping: resolved.keys) { resolved[$0]!.canonical }
                .filter { $0.value.count > 1 }
            guard collisions.isEmpty else {
                return fallback("Conflicting stored pane shortcuts were replaced by defaults.")
            }
            return PaneShortcutResolution(bindings: resolved, diagnostic: nil)
        } catch {
            return fallback("Invalid stored pane shortcuts were replaced by defaults: \(error.localizedDescription)")
        }
    }

    static func updating(
        command: PaneCommand,
        raw: String,
        current: [PaneCommand: PaneShortcut]
    ) -> Result<[PaneCommand: PaneShortcut], PaneShortcutValidationError> {
        do {
            let candidate = try PaneShortcut.parse(raw)
            if reserved.contains(candidate.canonical) {
                throw PaneShortcutValidationError.reserved(candidate.canonical)
            }
            if let conflict = current.first(where: {
                $0.key != command && $0.value == candidate
            }) {
                throw PaneShortcutValidationError.duplicate(conflict.key.title)
            }
            var updated = current
            updated[command] = candidate
            return .success(updated)
        } catch let error as PaneShortcutValidationError {
            return .failure(error)
        } catch {
            return .failure(.invalidFormat)
        }
    }

    static func command(
        for event: NSEvent,
        bindings: [PaneCommand: PaneShortcut]
    ) -> PaneCommand? {
        PaneCommand.allCases.first { command in
            (bindings[command] ?? command.defaultShortcut).matches(event)
        }
    }

    private static func fallback(_ diagnostic: String) -> PaneShortcutResolution {
        PaneShortcutResolution(bindings: defaults, diagnostic: diagnostic)
    }
}

enum UnifiedTabShortcutPolicy {
    static func isClose(_ event: NSEvent) -> Bool {
        PaneShortcut(key: "w", modifiers: [.command]).matches(event)
    }
}

enum PaneKeyEventPolicy {
    private static let chordModifiers: NSEvent.ModifierFlags = [
        .command, .control, .option, .shift,
    ]

    /// Cmd+E is an app-level navigation command. Intercepting it before the
    /// responder chain prevents a focused file editor from claiming macOS's
    /// default "Use Selection for Find" equivalent instead.
    static func isSidebarViewToggle(_ event: NSEvent) -> Bool {
        event.type == .keyDown
            && event.keyCode == 14
            && event.modifierFlags.intersection(chordModifiers) == .command
    }

    static func isAgentSwitcherAdvance(_ event: NSEvent) -> Bool {
        event.type == .keyDown
            && event.keyCode == 48
            && event.modifierFlags.intersection(chordModifiers) == .option
    }

    /// Option+Shift+Tab, the reverse of the chord above. Shift is the only
    /// added modifier, so Command or Control still falls through to whatever
    /// owns that chord.
    static func isAgentSwitcherRetreat(_ event: NSEvent) -> Bool {
        event.type == .keyDown
            && event.keyCode == 48
            && event.modifierFlags.intersection(chordModifiers) == [.option, .shift]
    }

    static func isTabSwitcherAdvance(_ event: NSEvent) -> Bool {
        event.type == .keyDown
            && event.keyCode == 48
            && event.modifierFlags.intersection(chordModifiers) == .control
    }

    /// Control+Shift+Tab is the reverse checkout-local tab chord. Command or
    /// Option keeps ownership of its own shortcut instead of being swallowed.
    static func isTabSwitcherRetreat(_ event: NSEvent) -> Bool {
        event.type == .keyDown
            && event.keyCode == 48
            && event.modifierFlags.intersection(chordModifiers) == [.control, .shift]
    }

    /// Accessibility synthesizers can emit the Tab key pair with Control in
    /// each event but omit the later Control flagsChanged event. The global
    /// modifier state is authoritative for that release fallback. A physical
    /// user holding Control keeps the switcher open and continues cycling.
    static func shouldCommitTabSwitcherAfterKeyUp(
        _ event: NSEvent,
        currentModifiers: NSEvent.ModifierFlags
    ) -> Bool {
        event.type == .keyUp
            && event.keyCode == 48
            && !currentModifiers.contains(.control)
    }
}

enum PaneMenuPolicy {
    static func reserveCloseShortcut(in menu: NSMenu) -> Bool {
        for item in menu.items {
            let modifiers = item.keyEquivalentModifierMask.intersection(.deviceIndependentFlagsMask)
            if item.title == "Close Window",
               item.keyEquivalent.lowercased() == "w",
               modifiers == .command {
                item.keyEquivalent = ""
                item.keyEquivalentModifierMask = []
                return true
            }
            if let submenu = item.submenu, reserveCloseShortcut(in: submenu) {
                return true
            }
        }
        return false
    }
}

enum PaneScrollPolicy {
    static func routesToLocalScroll(_ event: NSEvent) -> Bool {
        event.type == .scrollWheel
            && !event.modifierFlags
                .intersection(.deviceIndependentFlagsMask)
                .contains(.option)
    }

    /// Whole terminal rows a wheel event moves the pane.
    ///
    /// Precise (trackpad) deltas are pixel values, so they accumulate across
    /// events and keep their remainder; a classic wheel notch already arrives
    /// in line units and must always move at least one row rather than
    /// rounding away to nothing.
    static func rows(
        forDelta delta: CGFloat,
        precise: Bool,
        rowHeight: CGFloat,
        accumulator: inout CGFloat
    ) -> Int {
        guard rowHeight > 0 else { return 0 }
        guard precise else {
            accumulator = 0
            let rounded = Int(delta.rounded())
            if rounded != 0 { return rounded }
            return delta > 0 ? 1 : (delta < 0 ? -1 : 0)
        }
        accumulator += delta
        let rows = Int(accumulator / rowHeight)
        accumulator -= CGFloat(rows) * rowHeight
        return rows
    }
}

@MainActor
final class PaneCommandWindow: NSWindow {
    weak var paneCommandModel: ShellModel?
    /// Pixel remainder carried between precise scroll events, so a slow
    /// trackpad drag still adds up to whole rows instead of being discarded.
    private var scrollAccumulator: CGFloat = 0

    /// One terminal row in points, measured from the grid the view is showing.
    private func rowHeight(of terminal: TerminalView) -> CGFloat {
        let rows = CGFloat(terminal.terminal.rows)
        guard rows > 0, terminal.bounds.height > 0 else { return 0 }
        return terminal.bounds.height / rows
    }

    override func sendEvent(_ event: NSEvent) {
        guard paneCommandModel != nil else {
            super.sendEvent(event)
            return
        }
        if PaneScrollPolicy.routesToLocalScroll(event),
           let terminal = terminalView(at: event.locationInWindow),
           let paneID = (terminal as? any HideTerminalPointerRouting)?.hidePaneID,
           let model = paneCommandModel
        {
            // Herdr renders this pane and keeps its history, so no row ever
            // scrolls off the local grid and a local scrollback stays empty.
            // Forward the wheel instead and let Herdr answer with a frame,
            // which is the path its own TUI takes.
            let rows = PaneScrollPolicy.rows(
                forDelta: event.scrollingDeltaY,
                precise: event.hasPreciseScrollingDeltas,
                rowHeight: rowHeight(of: terminal),
                accumulator: &scrollAccumulator
            )
            if rows != 0 {
                model.core.scrollTerminal(
                    paneID: paneID,
                    direction: rows > 0 ? "up" : "down",
                    lines: abs(rows)
                )
            }
            return
        }
        super.sendEvent(event)
    }

    func terminalView(at windowPoint: NSPoint) -> TerminalView? {
        guard let contentView else { return nil }
        // NSEvent.locationInWindow is already expressed in the window content
        // coordinate system. Converting it from nil applies another window-base
        // transform and can make a visible terminal miss hit testing.
        var candidate = contentView.hitTest(windowPoint)
        while let view = candidate {
            if let terminal = view as? TerminalView { return terminal }
            // A view that scrolls on its own keeps the wheel before any
            // terminal beneath it does. The explorer tree and the file viewer
            // are both NSScrollView-backed and cover the pane canvas.
            if view is NSScrollView { return nil }
            candidate = view.superview
        }
        // `hitTest` landed on something that neither scrolls nor belongs to a
        // terminal: a resize strip, a status chip, any decoration that takes
        // clicks. Walking up from one can never reach the terminal, because a
        // decoration is the terminal's sibling and not its child, so the wheel
        // died wherever one was layered. Ask which terminal actually covers the
        // point instead, which keeps every future decoration transparent to
        // scrolling without each one having to opt in.
        return Self.frontmostTerminalView(in: contentView, containing: windowPoint)
    }

    /// Frames are compared in window coordinates because that is what
    /// `NSEvent.locationInWindow` already is; converting the point downward
    /// instead would reintroduce the transform noted above.
    static func frontmostTerminalView(
        in root: NSView,
        containing windowPoint: NSPoint
    ) -> TerminalView? {
        // Later siblings draw on top of earlier ones, so look front to back.
        for subview in root.subviews.reversed() {
            guard !subview.isHidden, subview.alphaValue > 0 else { continue }
            if let nested = frontmostTerminalView(in: subview, containing: windowPoint) {
                return nested
            }
            guard let terminal = subview as? TerminalView else { continue }
            if terminal.convert(terminal.bounds, to: nil).contains(windowPoint) {
                return terminal
            }
        }
        return nil
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if UnifiedTabShortcutPolicy.isClose(event), let model = paneCommandModel {
            model.performCloseShortcut()
            return true
        }
        if let model = paneCommandModel,
           let command = PaneShortcutPolicy.command(
               for: event,
               bindings: model.paneShortcuts
            ) {
            model.performPaneCommand(command)
            return true
        }
        return super.performKeyEquivalent(with: event)
    }
}

struct AppSettingsView: View {
    @ObservedObject var model: ShellModel

    var body: some View {
        Form {
            Section("Keyboard") {
                ForEach(PaneCommand.allCases) { command in
                    PaneShortcutRow(command: command, model: model)
                }
                if let diagnostic = model.shortcutDiagnostic {
                    Label(diagnostic, systemImage: "exclamationmark.triangle")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 520, height: 470)
        .padding(12)
    }
}

private struct PaneShortcutRow: View {
    let command: PaneCommand
    @ObservedObject var model: ShellModel
    @State private var draft: String

    init(command: PaneCommand, model: ShellModel) {
        self.command = command
        self.model = model
        _draft = State(initialValue: model.shortcut(for: command).canonical)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(command.title)
                    .frame(width: 120, alignment: .leading)
                TextField("command+d", text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .labelsHidden()
                    .accessibilityLabel("\(command.title) shortcut")
                    .onSubmit { model.updateShortcut(command, raw: draft) }
                Button("Apply") { model.updateShortcut(command, raw: draft) }
            }
            if let error = model.shortcutErrors[command] {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
        .onChange(of: model.shortcut(for: command).canonical) { _, value in
            draft = value
        }
    }
}
