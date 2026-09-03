import AppKit
import Carbon.HIToolbox

/// A global shortcut for the pet's visibility toggle.
///
/// Identity is the **physical** key, never the produced character: with
/// Option held macOS reports `π` for the P key, and an accelerator built from
/// that character binds to nothing and silently never fires. `NSEvent.keyCode`
/// is the physical code, so that is what is captured and stored.
struct PetHotkey: Equatable {
    let keyCode: UInt16
    let modifiers: Set<PaneShortcut.Modifier>

    /// Stored form, spelled the same way pane shortcuts are stored.
    var canonical: String {
        let ordered = PaneShortcut.Modifier.allCases
            .filter(modifiers.contains)
            .map(\.rawValue)
        return (ordered + [PetHotkey.name(for: keyCode) ?? "key\(keyCode)"])
            .joined(separator: "+")
    }

    /// macOS-facing label, e.g. `⌘⌥P`.
    var displayGlyphs: String {
        let glyphs: [PaneShortcut.Modifier: String] = [
            .control: "⌃",
            .option: "⌥",
            .shift: "⇧",
            .command: "⌘",
        ]
        let ordered = [PaneShortcut.Modifier.control, .option, .shift, .command]
            .filter(modifiers.contains)
            .compactMap { glyphs[$0] }
            .joined()
        return ordered + (PetHotkey.name(for: keyCode)?.uppercased() ?? "?")
    }

    /// Builds a hotkey from a key press, or returns `nil` while the press is
    /// not a complete binding yet.
    ///
    /// Two presses are rejected on purpose: a modifier alone (the user is
    /// still reaching for the key) and a bare key with no modifier at all,
    /// which would swallow that key system-wide - typing "p" anywhere would
    /// hide the pet.
    static func capture(from event: NSEvent) -> PetHotkey? {
        guard event.type == .keyDown else { return nil }
        var modifiers = Set<PaneShortcut.Modifier>()
        let flags = event.modifierFlags
        if flags.contains(.command) { modifiers.insert(.command) }
        if flags.contains(.control) { modifiers.insert(.control) }
        if flags.contains(.option) { modifiers.insert(.option) }
        if flags.contains(.shift) { modifiers.insert(.shift) }
        guard !modifiers.isEmpty else { return nil }
        guard name(for: event.keyCode) != nil else { return nil }
        return PetHotkey(keyCode: event.keyCode, modifiers: modifiers)
    }

    static func parse(_ raw: String) throws -> PetHotkey {
        let components = raw
            .lowercased()
            .replacingOccurrences(of: " ", with: "")
            .split(separator: "+", omittingEmptySubsequences: false)
            .map(String.init)
        guard components.count >= 2, let rawKey = components.last, !rawKey.isEmpty else {
            throw PetHotkeyError.invalidFormat
        }
        var modifiers = Set<PaneShortcut.Modifier>()
        for component in components.dropLast() {
            let modifier: PaneShortcut.Modifier? = switch component {
            case "cmd", "command": .command
            case "ctrl", "control": .control
            case "opt", "option", "alt": .option
            case "shift": .shift
            default: nil
            }
            guard let modifier, modifiers.insert(modifier).inserted else {
                throw PetHotkeyError.invalidFormat
            }
        }
        guard !modifiers.isEmpty else { throw PetHotkeyError.modifierRequired }
        guard let keyCode = keyCode(for: rawKey) else { throw PetHotkeyError.unsupportedKey }
        return PetHotkey(keyCode: keyCode, modifiers: modifiers)
    }

    var carbonModifiers: UInt32 {
        var result: UInt32 = 0
        if modifiers.contains(.command) { result |= UInt32(cmdKey) }
        if modifiers.contains(.option) { result |= UInt32(optionKey) }
        if modifiers.contains(.control) { result |= UInt32(controlKey) }
        if modifiers.contains(.shift) { result |= UInt32(shiftKey) }
        return result
    }

    // MARK: - Physical key table

    /// Physical keys this binding accepts, by virtual key code. Letters and
    /// digits are listed by their US-layout position, which is what a virtual
    /// key code means; the named keys carry no character at all.
    static let keyNames: [UInt16: String] = [
        UInt16(kVK_ANSI_A): "a", UInt16(kVK_ANSI_B): "b", UInt16(kVK_ANSI_C): "c",
        UInt16(kVK_ANSI_D): "d", UInt16(kVK_ANSI_E): "e", UInt16(kVK_ANSI_F): "f",
        UInt16(kVK_ANSI_G): "g", UInt16(kVK_ANSI_H): "h", UInt16(kVK_ANSI_I): "i",
        UInt16(kVK_ANSI_J): "j", UInt16(kVK_ANSI_K): "k", UInt16(kVK_ANSI_L): "l",
        UInt16(kVK_ANSI_M): "m", UInt16(kVK_ANSI_N): "n", UInt16(kVK_ANSI_O): "o",
        UInt16(kVK_ANSI_P): "p", UInt16(kVK_ANSI_Q): "q", UInt16(kVK_ANSI_R): "r",
        UInt16(kVK_ANSI_S): "s", UInt16(kVK_ANSI_T): "t", UInt16(kVK_ANSI_U): "u",
        UInt16(kVK_ANSI_V): "v", UInt16(kVK_ANSI_W): "w", UInt16(kVK_ANSI_X): "x",
        UInt16(kVK_ANSI_Y): "y", UInt16(kVK_ANSI_Z): "z",
        UInt16(kVK_ANSI_0): "0", UInt16(kVK_ANSI_1): "1", UInt16(kVK_ANSI_2): "2",
        UInt16(kVK_ANSI_3): "3", UInt16(kVK_ANSI_4): "4", UInt16(kVK_ANSI_5): "5",
        UInt16(kVK_ANSI_6): "6", UInt16(kVK_ANSI_7): "7", UInt16(kVK_ANSI_8): "8",
        UInt16(kVK_ANSI_9): "9",
        UInt16(kVK_ANSI_Equal): "=", UInt16(kVK_ANSI_Minus): "-",
        UInt16(kVK_Space): "space", UInt16(kVK_Return): "return",
        UInt16(kVK_Escape): "escape", UInt16(kVK_Tab): "tab",
        UInt16(kVK_LeftArrow): "left", UInt16(kVK_RightArrow): "right",
        UInt16(kVK_UpArrow): "up", UInt16(kVK_DownArrow): "down",
        UInt16(kVK_F1): "f1", UInt16(kVK_F2): "f2", UInt16(kVK_F3): "f3",
        UInt16(kVK_F4): "f4", UInt16(kVK_F5): "f5", UInt16(kVK_F6): "f6",
        UInt16(kVK_F7): "f7", UInt16(kVK_F8): "f8", UInt16(kVK_F9): "f9",
        UInt16(kVK_F10): "f10", UInt16(kVK_F11): "f11", UInt16(kVK_F12): "f12",
    ]

    static func name(for keyCode: UInt16) -> String? {
        keyNames[keyCode]
    }

    static func keyCode(for name: String) -> UInt16? {
        keyNames.first { $0.value == name }?.key
    }
}

enum PetHotkeyError: Error, Equatable, LocalizedError {
    case invalidFormat
    case modifierRequired
    case unsupportedKey
    case registrationConflict(String)

    var errorDescription: String? {
        switch self {
        case .invalidFormat:
            "Use a chord such as command+option+p."
        case .modifierRequired:
            "A global shortcut needs at least one modifier, or it would swallow that key everywhere."
        case .unsupportedKey:
            "Use a letter, digit, function key, arrow, space, return, tab, or escape."
        case let .registrationConflict(binding):
            "macOS refused \(binding): another application already owns that shortcut."
        }
    }
}

/// Carbon dispatches hot keys through a C handler that carries only the
/// numeric id, so the id-to-action mapping has to live outside the instance.
/// The box makes that shared state explicit and lets teardown reach it from a
/// nonisolated deinit.
private final class PetHotkeyRegistry: @unchecked Sendable {
    static let shared = PetHotkeyRegistry()

    private let lock = NSLock()
    private var handlers: [UInt32: () -> Void] = [:]
    private var nextIdentifier: UInt32 = 1

    func reserveIdentifier() -> UInt32 {
        lock.lock()
        defer { lock.unlock() }
        let identifier = nextIdentifier
        nextIdentifier += 1
        return identifier
    }

    func register(_ identifier: UInt32, action: @escaping () -> Void) {
        lock.lock()
        handlers[identifier] = action
        lock.unlock()
    }

    func remove(_ identifier: UInt32) {
        lock.lock()
        handlers.removeValue(forKey: identifier)
        lock.unlock()
    }

    func handler(for identifier: UInt32) -> (() -> Void)? {
        lock.lock()
        defer { lock.unlock() }
        return handlers[identifier]
    }
}

/// Registers the pet's global shortcut through Carbon.
///
/// Carbon is the right boundary here rather than a global `NSEvent` monitor:
/// it needs no Accessibility permission and it *reports a conflict* instead
/// of registering a shortcut that quietly never fires. An unset shortcut
/// registers nothing at all (D-14).
@MainActor
final class PetHotkeyRegistrar {
    /// The Carbon handle and its registry id, held together so teardown can
    /// release both from a nonisolated deinit.
    private final class Registration: @unchecked Sendable {
        var hotKeyRef: EventHotKeyRef?
        var identifier: UInt32?

        func release() {
            if let hotKeyRef {
                UnregisterEventHotKey(hotKeyRef)
                self.hotKeyRef = nil
            }
            if let identifier {
                PetHotkeyRegistry.shared.remove(identifier)
                self.identifier = nil
            }
        }

        deinit {
            release()
        }
    }

    private let registration = Registration()
    private var handlerInstalled = false
    private(set) var registered: PetHotkey?
    private(set) var lastError: String?

    private let action: () -> Void

    init(action: @escaping () -> Void) {
        self.action = action
    }


    /// Applies the stored accelerator. Passing the same value twice leaves
    /// exactly one registration behind.
    @discardableResult
    func apply(accelerator: String?) -> String? {
        let trimmed = accelerator?.trimmingCharacters(in: .whitespaces)
        guard let trimmed, !trimmed.isEmpty else {
            unregister()
            registered = nil
            lastError = nil
            return nil
        }
        let hotkey: PetHotkey
        do {
            hotkey = try PetHotkey.parse(trimmed)
        } catch {
            unregister()
            registered = nil
            lastError = (error as? LocalizedError)?.errorDescription ?? "\(error)"
            return lastError
        }
        if registered == hotkey, registration.hotKeyRef != nil, lastError == nil {
            return nil
        }
        unregister()
        installHandlerIfNeeded()

        let identifier = PetHotkeyRegistry.shared.reserveIdentifier()
        var reference: EventHotKeyRef?
        let hotKeyID = EventHotKeyID(signature: OSType(0x48_50_45_54), id: identifier)
        let status = RegisterEventHotKey(
            UInt32(hotkey.keyCode),
            hotkey.carbonModifiers,
            hotKeyID,
            GetEventDispatcherTarget(),
            0,
            &reference
        )
        guard status == noErr, let reference else {
            registered = nil
            lastError = PetHotkeyError.registrationConflict(hotkey.canonical).errorDescription
            return lastError
        }
        registration.hotKeyRef = reference
        registration.identifier = identifier
        PetHotkeyRegistry.shared.register(identifier) { [weak self] in
            MainActor.assumeIsolated { self?.action() }
        }
        registered = hotkey
        lastError = nil
        return nil
    }

    private func unregister() {
        registration.release()
    }

    private func installHandlerIfNeeded() {
        guard !handlerInstalled else { return }
        handlerInstalled = true
        var eventType = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard),
            eventKind: UInt32(kEventHotKeyPressed)
        )
        InstallEventHandler(
            GetEventDispatcherTarget(),
            { _, event, _ -> OSStatus in
                var hotKeyID = EventHotKeyID()
                let status = GetEventParameter(
                    event,
                    EventParamName(kEventParamDirectObject),
                    EventParamType(typeEventHotKeyID),
                    nil,
                    MemoryLayout<EventHotKeyID>.size,
                    nil,
                    &hotKeyID
                )
                guard status == noErr else { return status }
                let identifier = hotKeyID.id
                DispatchQueue.main.async {
                    PetHotkeyRegistry.shared.handler(for: identifier)?()
                }
                return noErr
            },
            1,
            &eventType,
            nil,
            nil
        )
    }
}
