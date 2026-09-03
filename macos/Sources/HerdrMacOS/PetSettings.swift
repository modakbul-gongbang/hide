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
            // The shell's own tokens: this view sits inside Settings, and the
            // system's accent, text background, and separator read as a
            // different application next to the rows around it.
            let background = isCapturing ? HideTheme.Native.elevated : HideTheme.Native.panel
            background.setFill()
            let path = NSBezierPath(
                roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5),
                xRadius: HideTheme.radiusSmall,
                yRadius: HideTheme.radiusSmall
            )
            path.fill()
            (isCapturing ? HideTheme.Native.secondary : HideTheme.Native.divider).setStroke()
            path.stroke()
            let attributes: [NSAttributedString.Key: Any] = [
                .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .medium),
                .foregroundColor: isCapturing
                    ? HideTheme.Native.secondary
                    : HideTheme.Native.primary,
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
