import AppKit
import SwiftUI

/// Captures one key press as a global shortcut.
///
/// The press is read as a physical key, so a chord that macOS renders as a
/// different character under Option still binds to the key the user actually
/// pressed.
struct PetShortcutCaptureField: NSViewRepresentable {
    @Binding var capturing: Bool
    let current: String?
    let onCapture: (PetHotkey) -> Void

    func makeNSView(context: Context) -> CaptureView {
        let view = CaptureView()
        view.onCapture = { hotkey in
            onCapture(hotkey)
            capturing = false
        }
        view.onCancel = { capturing = false }
        return view
    }

    func updateNSView(_ nsView: CaptureView, context: Context) {
        nsView.label = capturing
            ? "Press a shortcut…"
            : (current.flatMap { try? PetHotkey.parse($0) }?.displayGlyphs ?? "Not set")
        nsView.isCapturing = capturing
        nsView.needsDisplay = true
    }

    static func dismantleNSView(_ nsView: CaptureView, coordinator: ()) {
        nsView.isCapturing = false
    }

    final class CaptureView: NSView {
        var onCapture: ((PetHotkey) -> Void)?
        var onCancel: (() -> Void)?
        var label = "Not set"

        /// While recording, every key press this app receives is claimed by a
        /// local event monitor rather than by first-responder focus.
        ///
        /// Focus does not work here: SwiftUI owns the focus ring inside its
        /// own hosting hierarchy and takes it straight back, so the view
        /// never sees `keyDown`. A local monitor sees the press before
        /// dispatch - including Command chords, which would otherwise be
        /// swallowed by the key-equivalent chain and could fire a menu item -
        /// and returning nil consumes it.
        var isCapturing = false {
            didSet {
                guard isCapturing != oldValue else { return }
                if isCapturing { startMonitoring() } else { stopMonitoring() }
            }
        }

        /// Holds the monitor token so teardown can remove it from a
        /// nonisolated deinit.
        private final class Lifecycle: @unchecked Sendable {
            var monitor: Any?

            func remove() {
                if let monitor {
                    NSEvent.removeMonitor(monitor)
                    self.monitor = nil
                }
            }

            deinit {
                remove()
            }
        }

        private let lifecycle = Lifecycle()

        private static func log(_ kind: String) {
            FileHandle.standardError.write(Data("""
            {"component":"pet","kind":"shortcut.\(kind)"}

            """.utf8))
        }

        private func startMonitoring() {
            guard lifecycle.monitor == nil else { return }
            Self.log("capture_started")
            lifecycle.monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown]) { [weak self] event in
                guard let self, self.isCapturing else { return event }
                if event.keyCode == 53 {
                    self.onCancel?()
                    return nil
                }
                Self.log("key_seen")
                guard let hotkey = PetHotkey.capture(from: event) else {
                    // Not a complete binding yet: a modifier alone, or a bare
                    // key that would swallow that key system-wide. Consume it
                    // so a half-press cannot leak into a menu while recording.
                    return nil
                }
                self.onCapture?(hotkey)
                return nil
            }
        }

        private func stopMonitoring() {
            Self.log("capture_stopped")
            lifecycle.remove()
        }

        override func draw(_ dirtyRect: NSRect) {
            let background = isCapturing
                ? NSColor.controlAccentColor.withAlphaComponent(0.18)
                : NSColor.textBackgroundColor
            background.setFill()
            let path = NSBezierPath(roundedRect: bounds, xRadius: 5, yRadius: 5)
            path.fill()
            NSColor.separatorColor.setStroke()
            path.stroke()
            let attributes: [NSAttributedString.Key: Any] = [
                .font: NSFont.systemFont(ofSize: 12, weight: .medium),
                .foregroundColor: NSColor.labelColor,
            ]
            let text = label as NSString
            let size = text.size(withAttributes: attributes)
            text.draw(
                at: NSPoint(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2),
                withAttributes: attributes
            )
        }
    }
}

/// The Pet section of Settings: the same visibility state the menu bar,
/// shortcut, and URL scheme write, plus the shortcut itself.
struct PetSettingsSection: View {
    @ObservedObject var model: ShellModel
    @State private var capturing = false

    private var pet: CorePetSnapshot? { model.core.pet }

    var body: some View {
        Section("Pet") {
            Toggle(
                "Show pet",
                isOn: Binding(
                    get: { pet?.visible ?? true },
                    set: { model.core.setPetVisible($0) }
                )
            )
            .accessibilityIdentifier("pet-visible-toggle")

            HStack {
                Text("Toggle shortcut")
                    .frame(width: 120, alignment: .leading)
                PetShortcutCaptureField(
                    capturing: $capturing,
                    current: pet?.shortcut
                ) { hotkey in
                    model.updatePetShortcut(hotkey.canonical)
                }
                .frame(height: 24)
                .accessibilityIdentifier("pet-shortcut-field")
                Button(capturing ? "Cancel" : "Record") { capturing.toggle() }
                Button("Clear") {
                    capturing = false
                    model.updatePetShortcut(nil)
                }
                .disabled(pet?.shortcut == nil)
            }

            if let error = pet?.shortcutError {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.caption)
                    .foregroundStyle(.red)
                    .accessibilityIdentifier("pet-shortcut-error")
            } else if pet?.shortcut == nil {
                Text("No shortcut is registered. The menu bar, this toggle, and herdr-ide://toggle still work.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("pet-shortcut-unset")
            }

            LabeledContent("Connection") {
                Label(
                    pet?.connectionMessage ?? (pet?.connection ?? "unknown"),
                    systemImage: (pet?.isConnected ?? false)
                        ? "checkmark.circle.fill"
                        : "exclamationmark.triangle.fill"
                )
                .font(.caption)
                .foregroundStyle((pet?.isConnected ?? false) ? .green : .orange)
            }
            .accessibilityIdentifier("pet-connection-status")
        }
    }
}
