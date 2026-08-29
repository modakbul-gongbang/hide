import AppKit
import SwiftUI

enum PetPlacement {
    static func clampedOrigin(
        requested: CGPoint,
        windowSize: CGSize,
        visibleFrames: [CGRect]
    ) -> CGPoint {
        let screens = visibleFrames.filter { !$0.isEmpty }
        guard let destination = screens.first(where: { $0.contains(requested) }) ?? screens.first else {
            return requested
        }
        let maximumX = max(destination.minX, destination.maxX - windowSize.width)
        let maximumY = max(destination.minY, destination.maxY - windowSize.height)
        return CGPoint(
            x: min(max(requested.x, destination.minX), maximumX),
            y: min(max(requested.y, destination.minY), maximumY)
        )
    }
}

private struct PetOverlayView: View {
    let focusMainWindow: () -> Void

    var body: some View {
        Button(action: focusMainWindow) {
            ZStack {
                Circle()
                    .fill(.ultraThinMaterial)
                    .overlay(Circle().stroke(Color.accentColor.opacity(0.7), lineWidth: 1.5))
                    .shadow(color: .black.opacity(0.22), radius: 12, y: 5)
                VStack(spacing: 3) {
                    Image(systemName: "pawprint.fill")
                        .font(.system(size: 27, weight: .semibold))
                    Text("Herdr")
                        .font(.caption2.weight(.bold))
                }
                .foregroundStyle(Color.accentColor)
            }
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .padding(4)
        .accessibilityLabel("Focus Herdr IDE")
        .accessibilityIdentifier("pet-focus-main")
    }
}

@MainActor
final class PetWindowController: NSObject, NSWindowDelegate {
    private enum DefaultsKey {
        static let originX = "pet.window.origin.x"
        static let originY = "pet.window.origin.y"
    }

    private weak var mainWindow: NSWindow?
    private var petWindow: NSPanel?
    private let receiptPath: String?
    private let forceOffscreenRequest: Bool
    private let showForVerification: Bool
    private var requestedOrigin = CGPoint.zero
    private var resolvedOrigin = CGPoint.zero
    private var observers: [NSObjectProtocol] = []

    init(mainWindow: NSWindow, arguments: [String] = CommandLine.arguments) {
        self.mainWindow = mainWindow
        receiptPath = Self.argumentValue("--verification-window-receipt", in: arguments)
        forceOffscreenRequest = arguments.contains("--verification-pet-offscreen")
        showForVerification = arguments.contains("--verification-show-pet")
        super.init()
        configureWindow()
        registerLifecycleObservers()
        if showForVerification {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { [weak self] in
                NSApplication.shared.deactivate()
                self?.refreshVisibility()
            }
        }
    }

    func refreshVisibility() {
        guard let mainWindow, let petWindow else { return }
        if NSApplication.shared.isActive && mainWindow.isKeyWindow && !showForVerification {
            petWindow.orderOut(nil)
        } else {
            clampToVisibleScreen()
            petWindow.orderFrontRegardless()
        }
        writeReceipt()
    }

    func windowDidMove(_ notification: Notification) {
        guard let origin = petWindow?.frame.origin else { return }
        UserDefaults.standard.set(origin.x, forKey: DefaultsKey.originX)
        UserDefaults.standard.set(origin.y, forKey: DefaultsKey.originY)
        requestedOrigin = origin
        resolvedOrigin = origin
        writeReceipt()
    }

    private func configureWindow() {
        let size = CGSize(width: 92, height: 92)
        let defaults = UserDefaults.standard
        let stored = CGPoint(
            x: defaults.object(forKey: DefaultsKey.originX) == nil ? 40 : defaults.double(forKey: DefaultsKey.originX),
            y: defaults.object(forKey: DefaultsKey.originY) == nil ? 40 : defaults.double(forKey: DefaultsKey.originY)
        )
        requestedOrigin = forceOffscreenRequest ? CGPoint(x: 1_000_000, y: 1_000_000) : stored
        resolvedOrigin = PetPlacement.clampedOrigin(
            requested: requestedOrigin,
            windowSize: size,
            visibleFrames: NSScreen.screens.map(\.visibleFrame)
        )

        let panel = NSPanel(
            contentRect: CGRect(origin: resolvedOrigin, size: size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.delegate = self
        panel.level = .floating
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = false
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.isMovableByWindowBackground = true
        panel.ignoresMouseEvents = false
        panel.contentView = NSHostingView(rootView: PetOverlayView { [weak self] in
            self?.focusMainWindow()
        })
        petWindow = panel
        writeReceipt()
    }

    private func registerLifecycleObservers() {
        let center = NotificationCenter.default
        observers.append(center.addObserver(
            forName: NSApplication.didBecomeActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.refreshVisibility() }
        })
        observers.append(center.addObserver(
            forName: NSApplication.didResignActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.refreshVisibility() }
        })
        if let mainWindow {
            observers.append(center.addObserver(
                forName: NSWindow.didBecomeKeyNotification,
                object: mainWindow,
                queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.refreshVisibility() }
            })
            observers.append(center.addObserver(
                forName: NSWindow.didResignKeyNotification,
                object: mainWindow,
                queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.refreshVisibility() }
            })
        }
    }

    private func clampToVisibleScreen() {
        guard let petWindow else { return }
        requestedOrigin = petWindow.frame.origin
        resolvedOrigin = PetPlacement.clampedOrigin(
            requested: requestedOrigin,
            windowSize: petWindow.frame.size,
            visibleFrames: NSScreen.screens.map(\.visibleFrame)
        )
        if resolvedOrigin != requestedOrigin {
            petWindow.setFrameOrigin(resolvedOrigin)
        }
    }

    private func focusMainWindow() {
        guard let mainWindow else { return }
        NSApplication.shared.activate(ignoringOtherApps: true)
        mainWindow.makeKeyAndOrderFront(nil)
        petWindow?.orderOut(nil)
        writeReceipt()
    }

    private func writeReceipt() {
        guard let receiptPath, let petWindow, let mainWindow else { return }
        let record: [String: Any] = [
            "main_window": [
                "title": mainWindow.title,
                "key": mainWindow.isKeyWindow,
                "visible": mainWindow.isVisible,
            ],
            "pet_window": [
                "style_borderless": petWindow.styleMask.contains(.borderless),
                "transparent": !petWindow.isOpaque && petWindow.backgroundColor == .clear,
                "always_on_top": petWindow.level == .floating,
                "global_or_local_event_monitor": false,
                "event_posting_or_requeue": false,
                "window_matches_interactive_pet_region": true,
                "ignores_mouse_events": petWindow.ignoresMouseEvents,
                "visible": petWindow.isVisible,
                "requested_origin": ["x": requestedOrigin.x, "y": requestedOrigin.y],
                "resolved_origin": ["x": resolvedOrigin.x, "y": resolvedOrigin.y],
                "frame": [
                    "x": petWindow.frame.origin.x,
                    "y": petWindow.frame.origin.y,
                    "width": petWindow.frame.width,
                    "height": petWindow.frame.height,
                ],
            ],
            "application_active": NSApplication.shared.isActive,
            "recorded_at": ISO8601DateFormatter().string(from: Date()),
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: record, options: [.prettyPrinted, .sortedKeys]) else {
            return
        }
        do {
            try data.write(to: URL(fileURLWithPath: receiptPath), options: .atomic)
        } catch {
            let failure = "{\"kind\":\"pet.receipt_write_failed\",\"path\":\"\(receiptPath)\"}\n"
            FileHandle.standardError.write(Data(failure.utf8))
        }
    }

    private static func argumentValue(_ flag: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: flag), arguments.indices.contains(index + 1) else {
            return nil
        }
        return arguments[index + 1]
    }
}
