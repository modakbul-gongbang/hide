import AppKit
import Combine

struct HideTooltipState: Equatable {
    private(set) var hoveredID: String?
    private(set) var visibleID: String?
    private(set) var deadline: TimeInterval?

    mutating func enter(_ id: String, at now: TimeInterval) {
        guard hoveredID != id else { return }
        hoveredID = id
        visibleID = nil
        deadline = now + HideTheme.Hint.tooltipDelay
    }
    mutating func advance(to now: TimeInterval) {
        guard let deadline, now >= deadline else { return }
        visibleID = hoveredID
        self.deadline = nil
    }
    mutating func leave(_ id: String) {
        if hoveredID == id { dismiss() }
    }
    mutating func dismiss() { self = Self() }
    mutating func retain(_ ids: Set<String>) {
        if let hoveredID, !ids.contains(hoveredID) { dismiss() }
    }
    static func fadeDuration(reduceMotion: Bool) -> TimeInterval {
        reduceMotion ? 0 : HideTheme.Hint.fadeDuration
    }
    static func dismisses(_ type: NSEvent.EventType) -> Bool {
        [.leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel, .keyDown].contains(type)
    }
}

/// The geometry rule is shared by tooltips and held chips.
/// AppKit/SwiftUI report points, so window and anchor use the same coordinate space.
struct HideBalloonPlacement {
    static func center(anchor: CGRect, size: CGSize, window: CGSize) -> CGPoint {
        let inset = HideTheme.Hint.windowInset, gap = HideTheme.Hint.gap
        let width = min(size.width, max(0, window.width - inset * 2))
        let height = min(size.height, max(0, window.height - inset * 2))
        let above = anchor.minY - gap - height
        let y = above >= inset ? above : anchor.maxY + gap
        return CGPoint(
            x: min(max(anchor.midX - width / 2, inset), max(inset, window.width - inset - width)) + width / 2,
            y: min(max(y, inset), max(inset, window.height - inset - height)) + height / 2
        )
    }
}

@MainActor
final class HideTooltipController: ObservableObject {
    @Published private(set) var state = HideTooltipState()
    private var timer: Task<Void, Never>?
    private var monitor: Any?
    private var resignObserver: NSObjectProtocol?

    func hover(_ id: String, inside: Bool) {
        if inside { state.enter(id, at: ProcessInfo.processInfo.systemUptime) }
        else { state.leave(id) }
        timer?.cancel()
        guard let deadline = state.deadline else { return }
        timer = Task { [weak self] in
            let delay = max(0, deadline - ProcessInfo.processInfo.systemUptime)
            do { try await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000)) }
            catch { return }
            guard let self, !Task.isCancelled else { return }
            self.state.advance(to: ProcessInfo.processInfo.systemUptime)
        }
    }
    func remove(_ id: String) { state.leave(id) }
    func retain(_ ids: Set<String>) { state.retain(ids) }
    func dismiss() { timer?.cancel(); timer = nil; state.dismiss() }
    func start() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel, .keyDown]) { [weak self] event in
            MainActor.assumeIsolated {
                if HideTooltipState.dismisses(event.type) { self?.dismiss() }
            }
            return event
        }
        resignObserver = NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.dismiss() }
        }
    }
    func stop() {
        dismiss()
        if let monitor { NSEvent.removeMonitor(monitor) }
        if let resignObserver { NotificationCenter.default.removeObserver(resignObserver) }
        monitor = nil
        resignObserver = nil
    }
}
