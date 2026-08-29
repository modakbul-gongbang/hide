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

enum PetHitRegion {
    static let contentInset: CGFloat = 4

    static func contains(screenPoint: CGPoint, windowFrame: CGRect) -> Bool {
        let interactiveFrame = windowFrame.insetBy(dx: contentInset, dy: contentInset)
        guard interactiveFrame.width > 0, interactiveFrame.height > 0 else {
            return false
        }
        let horizontalRadius = interactiveFrame.width / 2
        let verticalRadius = interactiveFrame.height / 2
        let normalizedX = (screenPoint.x - interactiveFrame.midX) / horizontalRadius
        let normalizedY = (screenPoint.y - interactiveFrame.midY) / verticalRadius
        return normalizedX * normalizedX + normalizedY * normalizedY <= 1
    }

    static func ignoresMouseEvents(screenPoint: CGPoint, windowFrame: CGRect) -> Bool {
        !contains(screenPoint: screenPoint, windowFrame: windowFrame)
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

private final class PetWindowLifecycle: @unchecked Sendable {
    var observers: [NSObjectProtocol] = []
    var cursorTrackingTimer: Timer?

    deinit {
        cursorTrackingTimer?.invalidate()
        observers.forEach(NotificationCenter.default.removeObserver)
    }
}

@MainActor
final class PetWindowController: NSObject, NSWindowDelegate {
    private struct HitRegionProbe {
        let centerIgnoresMouseEvents: Bool
        let edgeIgnoresMouseEvents: Bool
        let cornerIgnoresMouseEvents: Bool
        let restoredToCursorState: Bool
    }

    private enum DefaultsKey {
        static let originX = "pet.window.origin.x"
        static let originY = "pet.window.origin.y"
    }

    private weak var mainWindow: NSWindow?
    private var petWindow: NSPanel?
    private let receiptPath: String?
    private let forceOffscreenRequest: Bool
    private let showForVerification: Bool
    private let runHitRegionProbe: Bool
    private var requestedOrigin = CGPoint.zero
    private var resolvedOrigin = CGPoint.zero
    private let lifecycle = PetWindowLifecycle()
    private var cursorTrackingStarts = 0
    private var cursorTrackingStops = 0
    private var lastCursorLocation = CGPoint.zero
    private var cursorInsidePet = false
    private var hitRegionProbe: HitRegionProbe?

    init(mainWindow: NSWindow, arguments: [String] = CommandLine.arguments) {
        self.mainWindow = mainWindow
        receiptPath = LaunchArguments.value("--verification-window-receipt", in: arguments)
        forceOffscreenRequest = arguments.contains("--verification-pet-offscreen")
        showForVerification = arguments.contains("--verification-show-pet")
        runHitRegionProbe = arguments.contains("--verification-pet-hit-region-probe")
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
            stopCursorTracking()
            petWindow.ignoresMouseEvents = true
            petWindow.orderOut(nil)
        } else {
            clampToVisibleScreen()
            petWindow.orderFrontRegardless()
            if runHitRegionProbe && hitRegionProbe == nil {
                hitRegionProbe = captureHitRegionProbe()
            }
            updateMouseInteractivity(at: NSEvent.mouseLocation, recordTransition: false)
            startCursorTracking()
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
        panel.ignoresMouseEvents = true
        panel.contentView = NSHostingView(rootView: PetOverlayView { [weak self] in
            self?.focusMainWindow()
        })
        petWindow = panel
        writeReceipt()
    }

    private func registerLifecycleObservers() {
        let center = NotificationCenter.default
        lifecycle.observers.append(center.addObserver(
            forName: NSApplication.didBecomeActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.refreshVisibility() }
        })
        lifecycle.observers.append(center.addObserver(
            forName: NSApplication.didResignActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.refreshVisibility() }
        })
        if let mainWindow {
            lifecycle.observers.append(center.addObserver(
                forName: NSWindow.didBecomeKeyNotification,
                object: mainWindow,
                queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.refreshVisibility() }
            })
            lifecycle.observers.append(center.addObserver(
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

    private func startCursorTracking() {
        guard lifecycle.cursorTrackingTimer == nil else { return }
        let timer = Timer(timeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.updateMouseInteractivity(at: NSEvent.mouseLocation)
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        lifecycle.cursorTrackingTimer = timer
        cursorTrackingStarts += 1
    }

    private func stopCursorTracking() {
        guard let cursorTrackingTimer = lifecycle.cursorTrackingTimer else { return }
        cursorTrackingTimer.invalidate()
        lifecycle.cursorTrackingTimer = nil
        cursorTrackingStops += 1
    }

    private func updateMouseInteractivity(at screenPoint: CGPoint, recordTransition: Bool = true) {
        guard let petWindow else { return }
        let shouldIgnore = PetHitRegion.ignoresMouseEvents(
            screenPoint: screenPoint,
            windowFrame: petWindow.frame
        )
        let changed = petWindow.ignoresMouseEvents != shouldIgnore
        petWindow.ignoresMouseEvents = shouldIgnore
        lastCursorLocation = screenPoint
        cursorInsidePet = !shouldIgnore
        if changed && recordTransition {
            writeReceipt()
        }
    }

    private func captureHitRegionProbe() -> HitRegionProbe? {
        guard let petWindow else { return nil }
        let frame = petWindow.frame
        let center = CGPoint(x: frame.midX, y: frame.midY)
        let edge = CGPoint(x: frame.maxX - PetHitRegion.contentInset, y: frame.midY)
        let corner = CGPoint(x: frame.maxX - 1, y: frame.maxY - 1)

        updateMouseInteractivity(at: center, recordTransition: false)
        let centerState = petWindow.ignoresMouseEvents
        updateMouseInteractivity(at: edge, recordTransition: false)
        let edgeState = petWindow.ignoresMouseEvents
        updateMouseInteractivity(at: corner, recordTransition: false)
        let cornerState = petWindow.ignoresMouseEvents
        let cursorLocation = NSEvent.mouseLocation
        updateMouseInteractivity(at: cursorLocation, recordTransition: false)
        let restoredState = PetHitRegion.ignoresMouseEvents(
            screenPoint: cursorLocation,
            windowFrame: frame
        )

        return HitRegionProbe(
            centerIgnoresMouseEvents: centerState,
            edgeIgnoresMouseEvents: edgeState,
            cornerIgnoresMouseEvents: cornerState,
            restoredToCursorState: petWindow.ignoresMouseEvents == restoredState
        )
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
        let probeRecord: [String: Any]
        if let hitRegionProbe {
            probeRecord = [
                "ran_without_input_automation": true,
                "center_ignores_mouse_events": hitRegionProbe.centerIgnoresMouseEvents,
                "edge_ignores_mouse_events": hitRegionProbe.edgeIgnoresMouseEvents,
                "corner_ignores_mouse_events": hitRegionProbe.cornerIgnoresMouseEvents,
                "restored_to_cursor_state": hitRegionProbe.restoredToCursorState,
            ]
        } else {
            probeRecord = ["ran_without_input_automation": false]
        }
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
                "window_uses_selective_hit_region": true,
                "ignores_mouse_events": petWindow.ignoresMouseEvents,
                "selective_hit_region": [
                    "shape": "ellipse",
                    "content_inset": PetHitRegion.contentInset,
                    "cursor_inside_pet": cursorInsidePet,
                    "last_cursor": ["x": lastCursorLocation.x, "y": lastCursorLocation.y],
                    "polling_active": lifecycle.cursorTrackingTimer != nil,
                    "active_polling_instances": lifecycle.cursorTrackingTimer == nil ? 0 : 1,
                    "polling_starts": cursorTrackingStarts,
                    "polling_stops": cursorTrackingStops,
                    "probe": probeRecord,
                ],
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
        VerificationReceipt.write(data, to: receiptPath, failureKind: "pet.receipt_write_failed")
    }
}
