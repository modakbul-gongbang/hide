import AppKit
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
        case .closePane: PaneShortcut(key: "w", modifiers: [.command])
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

@MainActor
final class PaneCommandWindow: NSWindow {
    weak var paneCommandModel: ShellModel?

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
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

struct KeyboardSettingsView: View {
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
        .frame(width: 520, height: 330)
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
