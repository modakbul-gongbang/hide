import AppKit
import Combine
import SwiftUI

enum PetPlacement {
    /// The pet is never allowed to sit where the user cannot reach it.
    ///
    /// A saved origin that lands on no connected screen (the recorded
    /// incident coordinate `[542720, 163840]`, or a display that has since
    /// been unplugged) resolves onto the primary visible frame instead of
    /// succeeding invisibly.
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

    /// Where a pet with no saved position starts.
    static let defaultOrigin = CGPoint(x: 40, y: 40)

    static let windowSize = CGSize(width: 128, height: 128)
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

/// Separates a click from a drag.
///
/// A release within the threshold of where the press started is a click; past
/// it the gesture was a move and must not also jump to a pane.
enum PetGesture {
    static let dragThreshold: CGFloat = 3

    static func isDrag(from start: CGPoint, to end: CGPoint) -> Bool {
        hypot(end.x - start.x, end.y - start.y) > dragThreshold
    }

    /// The window origin that keeps the cursor at the same point on the pet.
    ///
    /// Anchoring on the offset captured at press time - rather than
    /// accumulating per-event deltas - is what stops the pet drifting out
    /// from under the cursor over a long drag.
    static func anchoredOrigin(cursor: CGPoint, anchor: CGSize) -> CGPoint {
        CGPoint(x: cursor.x - anchor.width, y: cursor.y - anchor.height)
    }
}

/// Owns pointer handling for the pet.
///
/// The panel is non-activating and never key, so the view has to accept the
/// first mouse itself; without that the first click after using another app
/// is swallowed by activation.
final class PetInteractionView: NSView {
    var onPressed: (() -> Void)?
    var onDragged: ((CGPoint) -> Void)?
    var onDragEnded: (() -> Void)?
    var onClicked: (() -> Void)?

    private var pressLocation: CGPoint?
    private var anchor: CGSize = .zero
    private var dragging = false

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        guard let window else { return }
        let cursor = NSEvent.mouseLocation
        pressLocation = cursor
        anchor = CGSize(
            width: cursor.x - window.frame.origin.x,
            height: cursor.y - window.frame.origin.y
        )
        dragging = false
        onPressed?()
    }

    override func mouseDragged(with event: NSEvent) {
        guard let pressLocation else { return }
        let cursor = NSEvent.mouseLocation
        if !dragging, !PetGesture.isDrag(from: pressLocation, to: cursor) {
            return
        }
        dragging = true
        onDragged?(PetGesture.anchoredOrigin(cursor: cursor, anchor: anchor))
    }

    override func mouseUp(with event: NSEvent) {
        defer {
            pressLocation = nil
            dragging = false
        }
        if dragging {
            onDragEnded?()
        } else {
            onClicked?()
        }
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

/// Renders the pet and routes its gestures back into the core.
///
/// The core owns the pet state it expresses: pose, badge counts, visibility,
/// and placement. This controller draws that snapshot, separates click from
/// drag, and opens the shell dashboard; only a dashboard row dispatches pane
/// focus back through the core.
@MainActor
final class PetWindowController: NSObject, NSWindowDelegate {
    private struct HitRegionProbe {
        let centerIgnoresMouseEvents: Bool
        let edgeIgnoresMouseEvents: Bool
        let cornerIgnoresMouseEvents: Bool
        let restoredToCursorState: Bool
    }

    private weak var mainWindow: NSWindow?
    private let model: ShellModel
    private var petWindow: NSPanel?
    private let animator = PetAnimator()
    private let receiptPath: String?
    private let forceOffscreenRequest: Bool
    private let runHitRegionProbe: Bool
    private var requestedOrigin = CGPoint.zero
    private var resolvedOrigin = CGPoint.zero
    private let lifecycle = PetWindowLifecycle()
    private var cursorTrackingStarts = 0
    private var cursorTrackingStops = 0
    private var lastCursorLocation = CGPoint.zero
    private var cursorInsidePet = false
    private var hitRegionProbe: HitRegionProbe?
    private var snapshotSubscription: AnyCancellable?
    private var theme: PetTheme?
    private var themeError: String?
    private var frameCache: [String: [PetFrame]] = [:]
    private var renderedPose: String?
    private var renderedVisible: Bool?
    private var renderedBadges = CorePetBadges.none
    private var appliedOrigin: CGPoint?

    init(mainWindow: NSWindow, model: ShellModel, arguments: [String] = CommandLine.arguments) {
        self.mainWindow = mainWindow
        self.model = model
        receiptPath = LaunchArguments.value("--verification-window-receipt", in: arguments)
        forceOffscreenRequest = arguments.contains("--verification-pet-offscreen")
        runHitRegionProbe = arguments.contains("--verification-pet-hit-region-probe")
        super.init()
        loadTheme(arguments: arguments)
        configureWindow()
        registerLifecycleObservers()
        snapshotSubscription = model.core.objectWillChange.sink { [weak self] _ in
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.applySnapshot() }
            }
        }
        applySnapshot()
    }

    // MARK: - Theme

    private func loadTheme(arguments: [String]) {
        let overrideRoot = LaunchArguments.value("--pet-theme-root", in: arguments)
            .map { URL(fileURLWithPath: $0, isDirectory: true) }
        guard let root = overrideRoot
            ?? PetThemeLocator.themesRoot(repositoryRoot: model.core.workspaceRoot)
        else {
            themeError = "No pet theme directory was found beside the app or in the workspace."
            reportThemeFailure()
            return
        }
        do {
            theme = try PetTheme.load(themesRoot: root, id: "default")
        } catch {
            themeError = error.localizedDescription
            reportThemeFailure()
        }
    }

    /// An unloadable theme is stated on stderr and in the window, never
    /// swallowed into an empty pet.
    private func reportThemeFailure() {
        guard let themeError else { return }
        FileHandle.standardError.write(Data(("""
        {"component":"pet","kind":"theme.unavailable","message":"\(themeError.replacingOccurrences(of: "\"", with: "'"))"}

        """).utf8))
    }

    private func frames(for pose: String) -> [PetFrame]? {
        if let cached = frameCache[pose] { return cached }
        guard let state = theme?.state(for: pose) else { return nil }
        guard let loaded = try? PetAnimationLoader.frames(for: state), !loaded.isEmpty else {
            return nil
        }
        frameCache[pose] = loaded
        return loaded
    }

    // MARK: - Snapshot

    private func applySnapshot() {
        guard let pet = model.core.pet else { return }

        if renderedPose != pet.pose {
            renderedPose = pet.pose
            if let frames = frames(for: pet.pose) {
                animator.show(pose: pet.pose, frames: frames)
            } else {
                animator.clear(reason: themeError ?? "no art for pose \(pet.pose)")
            }
        }
        if renderedBadges != pet.badges {
            renderedBadges = pet.badges
            refreshContent()
        }
        // A position restored from the store still passes through the clamp,
        // so an origin saved on a display that is now unplugged comes back
        // somewhere visible.
        if let origin = pet.origin?.point, appliedOrigin != origin {
            appliedOrigin = origin
            moveWindow(to: origin, persist: false)
        }
        if renderedVisible != pet.visible {
            renderedVisible = pet.visible
            refreshVisibility()
        }
        writeReceipt()
    }

    private func refreshContent() {
        guard let petWindow else { return }
        let view = PetView(
            animator: animator,
            badges: renderedBadges,
            unavailableReason: theme == nil ? (themeError ?? "The pet theme is unavailable.") : nil
        )
        if let hosting = petWindow.contentView?.subviews.compactMap({ $0 as? NSHostingView<PetView> }).first {
            hosting.rootView = view
        }
    }

    /// Shows or hides the pet to match the single visibility state the four
    /// toggle surfaces share.
    func refreshVisibility() {
        guard let petWindow else { return }
        guard model.core.pet?.visible ?? true else {
            stopCursorTracking()
            petWindow.ignoresMouseEvents = true
            petWindow.orderOut(nil)
            writeReceipt()
            return
        }
        clampToVisibleScreen()
        petWindow.orderFrontRegardless()
        if runHitRegionProbe && hitRegionProbe == nil {
            hitRegionProbe = captureHitRegionProbe()
        }
        updateMouseInteractivity(at: NSEvent.mouseLocation, recordTransition: false)
        startCursorTracking()
        writeReceipt()
    }

    func windowDidMove(_ notification: Notification) {
        guard let origin = petWindow?.frame.origin else { return }
        requestedOrigin = origin
        resolvedOrigin = origin
        writeReceipt()
    }

    // MARK: - Window

    private func configureWindow() {
        let size = PetPlacement.windowSize
        let stored = model.core.pet?.origin?.point ?? PetPlacement.defaultOrigin
        requestedOrigin = forceOffscreenRequest ? CGPoint(x: 542_720, y: 163_840) : stored
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
        // A shadow on a transparent window draws a grey rounded backdrop
        // behind the pet on macOS.
        panel.hasShadow = false
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.ignoresMouseEvents = true

        let interaction = PetInteractionView(frame: CGRect(origin: .zero, size: size))
        interaction.autoresizingMask = [.width, .height]
        interaction.onPressed = { [weak self] in
            self?.model.core.notePetActivity()
        }
        interaction.onDragged = { [weak self] origin in
            self?.beginDragIfNeeded()
            self?.moveWindow(to: origin, persist: false)
        }
        interaction.onDragEnded = { [weak self] in
            self?.endDrag()
        }
        interaction.onClicked = { [weak self] in
            self?.handleClick()
        }

        let hosting = NSHostingView(
            rootView: PetView(
                animator: animator,
                badges: renderedBadges,
                unavailableReason: theme == nil ? (themeError ?? "The pet theme is unavailable.") : nil
            )
        )
        hosting.frame = CGRect(origin: .zero, size: size)
        hosting.autoresizingMask = [.width, .height]
        interaction.addSubview(hosting)
        panel.contentView = interaction
        petWindow = panel
        writeReceipt()
    }

    private var dragging = false

    private func beginDragIfNeeded() {
        guard !dragging else { return }
        dragging = true
        model.core.setPetDragging(true)
    }

    private func endDrag() {
        guard dragging else { return }
        dragging = false
        model.core.setPetDragging(false)
        if let origin = petWindow?.frame.origin {
            appliedOrigin = origin
            model.core.movePet(to: origin)
        }
    }

    /// A click raises the IDE and opens the snapshot-backed agent dashboard.
    private func handleClick() {
        model.core.notePetActivity()
        model.openPetDashboard()
        guard let mainWindow else { return }
        NSApplication.shared.activate(ignoringOtherApps: true)
        mainWindow.makeKeyAndOrderFront(nil)
    }

    private func moveWindow(to origin: CGPoint, persist: Bool) {
        guard let petWindow else { return }
        let clamped = PetPlacement.clampedOrigin(
            requested: origin,
            windowSize: petWindow.frame.size,
            visibleFrames: NSScreen.screens.map(\.visibleFrame)
        )
        requestedOrigin = origin
        resolvedOrigin = clamped
        petWindow.setFrameOrigin(clamped)
        if persist {
            appliedOrigin = clamped
            model.core.movePet(to: clamped)
        }
    }

    private func registerLifecycleObservers() {
        let center = NotificationCenter.default
        for name in [
            NSApplication.didBecomeActiveNotification,
            NSApplication.didResignActiveNotification,
        ] {
            lifecycle.observers.append(center.addObserver(
                forName: name,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.refreshVisibility() }
            })
        }
        // A display change can strand a saved origin outside every screen.
        lifecycle.observers.append(center.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.clampToVisibleScreen()
                if let origin = self.petWindow?.frame.origin, origin != self.appliedOrigin {
                    self.appliedOrigin = origin
                    self.model.core.movePet(to: origin)
                }
            }
        })
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
        // Pointer capture stays with the pet for the whole gesture; releasing
        // it mid-drag would leave the pet glued to the cursor.
        let shouldIgnore = dragging
            ? false
            : PetHitRegion.ignoresMouseEvents(screenPoint: screenPoint, windowFrame: petWindow.frame)
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
        let pet = model.core.pet
        let mainWindowRecord: [String: Any] = [
            "title": mainWindow.title,
            "key": mainWindow.isKeyWindow,
            "visible": mainWindow.isVisible,
        ]
        let badgeRecord: [String: Any] = [
            "working": pet?.badges.working ?? 0,
            "done": pet?.badges.done ?? 0,
            "attention": pet?.badges.attention ?? 0,
            "error": pet?.badges.error ?? 0,
            "disconnected": pet?.badges.disconnected ?? 0,
            "subagents_active": pet?.badges.subagentsActive ?? 0,
            "background_running": pet?.badges.backgroundRunning ?? 0,
            "background_failed": pet?.badges.backgroundFailed ?? 0,
        ]
        let petStateRecord: [String: Any] = [
            "pose": pet?.pose ?? "unknown",
            "sleep_phase": pet?.sleepPhase ?? "unknown",
            "connection": pet?.connection ?? "unknown",
            "connection_message": pet?.connectionMessage ?? "",
            "roam_allowed": pet?.roamAllowed ?? false,
            "visible": pet?.visible ?? false,
            "shortcut": pet?.shortcut ?? "",
            "shortcut_error": pet?.shortcutError ?? "",
            "theme_id": pet?.themeID ?? "",
            "theme_loaded": theme != nil,
            "theme_error": themeError ?? "",
            "attention_pane_ids": pet?.attentionPaneIDs ?? [],
            "badges": badgeRecord,
        ]
        let hitRegionRecord: [String: Any] = [
            "shape": "ellipse",
            "content_inset": PetHitRegion.contentInset,
            "cursor_inside_pet": cursorInsidePet,
            "last_cursor": ["x": lastCursorLocation.x, "y": lastCursorLocation.y],
            "polling_active": lifecycle.cursorTrackingTimer != nil,
            "active_polling_instances": lifecycle.cursorTrackingTimer == nil ? 0 : 1,
            "polling_starts": cursorTrackingStarts,
            "polling_stops": cursorTrackingStops,
            "probe": probeRecord,
        ]
        let frameRecord: [String: Any] = [
            "x": petWindow.frame.origin.x,
            "y": petWindow.frame.origin.y,
            "width": petWindow.frame.width,
            "height": petWindow.frame.height,
        ]
        let petWindowRecord: [String: Any] = [
            "style_borderless": petWindow.styleMask.contains(.borderless),
            "transparent": !petWindow.isOpaque && petWindow.backgroundColor == .clear,
            "always_on_top": petWindow.level == .floating,
            "shadow": petWindow.hasShadow,
            "window_uses_selective_hit_region": true,
            "ignores_mouse_events": petWindow.ignoresMouseEvents,
            "selective_hit_region": hitRegionRecord,
            "visible": petWindow.isVisible,
            "requested_origin": ["x": requestedOrigin.x, "y": requestedOrigin.y],
            "resolved_origin": ["x": resolvedOrigin.x, "y": resolvedOrigin.y],
            "frame": frameRecord,
        ]
        let record: [String: Any] = [
            "main_window": mainWindowRecord,
            "pet_state": petStateRecord,
            "pet_window": petWindowRecord,
            "application_active": NSApplication.shared.isActive,
            "recorded_at": ISO8601DateFormatter().string(from: Date()),
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: record, options: [.prettyPrinted, .sortedKeys]) else {
            return
        }
        VerificationReceipt.write(data, to: receiptPath, failureKind: "pet.receipt_write_failed")
    }
}
