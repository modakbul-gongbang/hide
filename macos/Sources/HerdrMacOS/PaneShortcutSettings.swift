import AppKit
import SwiftTerm
import SwiftUI

enum PaneCommand: String, CaseIterable, Hashable, Identifiable, Sendable {
    case splitRight = "split_right"
    case splitDown = "split_down"
    case toggleZoom = "toggle_zoom"
    case closePane = "close_pane"
    case increaseTextSize = "increase_text_size"
    case decreaseTextSize = "decrease_text_size"
    case resetTextSize = "reset_text_size"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .splitRight: "Split Right"
        case .splitDown: "Split Down"
        case .toggleZoom: "Toggle Zoom"
        case .closePane: "Close Pane"
        case .increaseTextSize: "Increase Text Size"
        case .decreaseTextSize: "Decrease Text Size"
        case .resetTextSize: "Actual Text Size"
        }
    }

    var defaultShortcut: PaneShortcut {
        switch self {
        case .splitRight: PaneShortcut(key: "d", modifiers: [.command])
        case .splitDown: PaneShortcut(key: "d", modifiers: [.command, .shift])
        case .toggleZoom: PaneShortcut(key: "return", modifiers: [.command, .option])
        case .closePane: PaneShortcut(key: "w", modifiers: [.command, .shift])
        // `toggleZoom` above is layout zoom - one pane filling the tab. These
        // three are text size, which is why they are not named zoom.
        case .increaseTextSize: PaneShortcut(key: "=", modifiers: [.command])
        case .decreaseTextSize: PaneShortcut(key: "-", modifiers: [.command])
        case .resetTextSize: PaneShortcut(key: "0", modifiers: [.command])
        }
    }

    /// The direction this command asks the core for, or nil when the command
    /// is not about text size at all.
    var textScaleDirection: PaneTextScaleDirection? {
        switch self {
        case .increaseTextSize: .in
        case .decreaseTextSize: .out
        case .resetTextSize: .reset
        case .splitRight, .splitDown, .toggleZoom, .closePane: nil
        }
    }
}

/// The wire values the core's `pane_text_scale` event accepts.
enum PaneTextScaleDirection: String, Sendable {
    case `in`
    case out
    case reset
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
        let symbols = HideTheme.modifierSymbols
        // macOS prints modifiers in a fixed order regardless of how they were
        // declared: control, option, shift, command.
        let order: [Modifier] = [.control, .option, .shift, .command]
        let prefix = order.filter(modifiers.contains).compactMap { symbols[$0] }.joined()
        return prefix + (key == "return" ? "↩" : key.uppercased())
    }

    var keyEquivalent: KeyEquivalent {
        if key == "tab" { return .tab }
        return key == "return" ? .return : KeyEquivalent(Character(key))
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
        if key == "tab" { return event.keyCode == 48 }
        if key == "return" {
            return event.keyCode == 36 || event.keyCode == 76
        }
        // Identity is the physical key, never the produced character. With a
        // Hangul, Kana, or Cyrillic input source selected, macOS reports the
        // character that source produces - the D key comes through as "ㅇ" -
        // so comparing `charactersIgnoringModifiers` leaves every chord in
        // this catalog dead for as long as that source is active, silently and
        // all at once. `PetHotkey` already learned this for the pet's own
        // binding and keys off the virtual code; this uses the same table so
        // there is one answer to "which physical key is this" in the app.
        return PetHotkey.name(for: event.keyCode) == key
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
    private static let reserved = Set(ShellMenuCommand.allCases.map { $0.shortcut.canonical })
        .union((1...9).flatMap { ["command+\($0)", "option+\($0)"] })
        .union([
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

    /// Option cycles projects globally; Control cycles every surface in the
    /// current project. The registry also supplies menus, hints and settings.
    static func isProjectSwitcherAdvance(_ event: NSEvent) -> Bool {
        ShellMenuCommand.recentProject.shortcut.matches(event)
    }

    static func isProjectSwitcherRetreat(_ event: NSEvent) -> Bool {
        ShellMenuCommand.previousRecentProject.shortcut.matches(event)
    }

    static func isTabSwitcherAdvance(_ event: NSEvent) -> Bool {
        ShellMenuCommand.recentTab.shortcut.matches(event)
    }

    static func isTabSwitcherRetreat(_ event: NSEvent) -> Bool {
        ShellMenuCommand.previousRecentTab.shortcut.matches(event)
    }

    static func isTabSwitcherRelease(_ event: NSEvent) -> Bool {
        event.type == .flagsChanged && !event.modifierFlags.contains(.control)
    }

    static func isProjectSwitcherRelease(_ event: NSEvent) -> Bool {
        event.type == .flagsChanged && !event.modifierFlags.contains(.option)
    }

    /// Some accessibility synthesizers omit flagsChanged after the Tab pair.
    /// Physical holds remain open until the corresponding global flag clears.
    static func shouldCommitTabSwitcherAfterKeyUp(
        _ event: NSEvent, currentModifiers: NSEvent.ModifierFlags
    ) -> Bool {
        event.type == .keyUp && event.keyCode == 48 && !currentModifiers.contains(.control)
    }

    static func shouldCommitProjectSwitcherAfterKeyUp(
        _ event: NSEvent, currentModifiers: NSEvent.ModifierFlags
    ) -> Bool {
        event.type == .keyUp && event.keyCode == 48 && !currentModifiers.contains(.option)
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
    /// Herdr's documented terminal.scroll modifiers use crossterm's bitset.
    static func modifiers(_ flags: NSEvent.ModifierFlags) -> Int {
        (flags.contains(.shift) ? 1 : 0)
            | (flags.contains(.control) ? 2 : 0)
            | (flags.contains(.option) ? 4 : 0)
            | (flags.contains(.command) ? 8 : 0)
    }

    static func routesToHerdrScroll(_ event: NSEvent) -> Bool {
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
    private enum WheelRoute { case terminal, appKit }
    private var wheelRoute: WheelRoute?
    private weak var wheelRouteTerminal: TerminalView?
    private var wheelRoutePoint: NSPoint?
    private var wheelRouteTimestamp: TimeInterval?
    private static let wheelRouteReuseInterval: TimeInterval = 0.1
    private static let wheelRoutePointTolerance: CGFloat = 1

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
        if event.type == .keyDown, let terminal = firstResponder as? ImeTerminalView,
           let paneID = terminal.hidePaneID {
            TerminalLatency.end(.keyToSend, paneID: paneID, outcome: "consumed")
            TerminalLatency.begin(.keyToSend, paneID: paneID)
        }
        if PaneScrollPolicy.routesToHerdrScroll(event),
           let terminal = terminalView(
               forWheelAt: event.locationInWindow,
               timestamp: event.timestamp
           ),
           let terminal = terminal as? ImeTerminalView,
           let paneID = terminal.hidePaneID,
           let model = paneCommandModel
        {
            let rows = PaneScrollPolicy.rows(
                forDelta: event.scrollingDeltaY,
                precise: event.hasPreciseScrollingDeltas,
                rowHeight: rowHeight(of: terminal),
                accumulator: &scrollAccumulator
            )
            if rows != 0 {
                TerminalLatency.begin(.wheelToDraw, paneID: paneID)
                let cell = terminal.mouseCell(with: event)
                model.core.scrollTerminal(
                    paneID: paneID, direction: rows > 0 ? "up" : "down", lines: abs(rows),
                    column: cell.column, row: cell.row,
                    modifiers: PaneScrollPolicy.modifiers(event.modifierFlags)
                )
            }
            return
        }
        super.sendEvent(event)
    }

    /// A trackpad gesture delivers many wheel events to the same point. The
    /// first event resolves the real AppKit target; later events reuse that
    /// result while they remain consecutive and stationary. This preserves
    /// exact overlay/scroller ownership but avoids asking SwiftUI to walk the
    /// same responder graph twice for every tick (once here, then again in
    /// `super.sendEvent`).
    func terminalView(
        forWheelAt windowPoint: NSPoint,
        timestamp: TimeInterval
    ) -> TerminalView? {
        if let previousPoint = wheelRoutePoint,
           let previousTimestamp = wheelRouteTimestamp,
           timestamp >= previousTimestamp,
           timestamp - previousTimestamp <= Self.wheelRouteReuseInterval,
           abs(windowPoint.x - previousPoint.x) <= Self.wheelRoutePointTolerance,
           abs(windowPoint.y - previousPoint.y) <= Self.wheelRoutePointTolerance,
           let wheelRoute
        {
            self.wheelRouteTimestamp = timestamp
            switch wheelRoute {
            case .appKit:
                return nil
            case .terminal:
                if let wheelRouteTerminal { return wheelRouteTerminal }
            }
        }

        let terminal = terminalView(at: windowPoint)
        wheelRoutePoint = windowPoint
        wheelRouteTimestamp = timestamp
        wheelRouteTerminal = terminal
        wheelRoute = terminal == nil ? .appKit : .terminal
        return terminal
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
