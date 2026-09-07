import AppKit
import Testing
@testable import HerdrMacOS

@Suite("Hide tooltip contract")
struct HideTooltipTests {
    @Test @MainActor func controllerRevealsAndClearsOnWindowResignation() async throws {
        let controller = HideTooltipController()
        controller.start()
        defer { controller.stop() }
        controller.hover("new-agent", inside: true)
        #expect(controller.state.visibleID == nil)
        // Other AppKit tests can occupy the main actor past both deadlines.
        // A fixed sleep can resume before the already-due reveal task runs.
        // Assert eventual delivery here; delayAndDismissalEvents checks the
        // exact 400 ms threshold without depending on scheduler timing.
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(3))
        while controller.state.visibleID == nil && clock.now < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(controller.state.visibleID == "new-agent")
        NotificationCenter.default.post(name: NSWindow.didResignKeyNotification, object: nil)
        #expect(controller.state.visibleID == nil)
        controller.hover("removed-pane", inside: true)
        controller.retain([])
        try await Task.sleep(for: .milliseconds(450))
        #expect(controller.state.visibleID == nil)
    }

    @Test func delayAndDismissalEvents() {
        var state = HideTooltipState()
        state.enter("new-agent", at: 0)
        state.advance(to: 0.399)
        #expect(state.visibleID == nil)
        state.advance(to: 0.400)
        #expect(state.visibleID == "new-agent")
        state.leave("other")
        #expect(state.visibleID == "new-agent")
        state.leave("new-agent")
        #expect(state.visibleID == nil)
        for type: NSEvent.EventType in [.leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel, .keyDown] {
            state.enter("close", at: 1)
            state.advance(to: 2)
            #expect(HideTooltipState.dismisses(type))
            if HideTooltipState.dismisses(type) { state.dismiss() }
            #expect(state.visibleID == nil && state.deadline == nil)
        }
        state.enter("close", at: 3)
        state.advance(to: 4)
        state.retain(["new-agent"])
        #expect(state.visibleID == nil && state.hoveredID == nil)
        state.enter("close", at: 5)
        state.advance(to: 6)
        state.dismiss() // window resign-key uses the same dismissal
        #expect(state.visibleID == nil)
        #expect(!HideTooltipState.dismisses(.flagsChanged))
    }

    @Test func replacementDoesNotRevealPreviousAnchor() {
        var state = HideTooltipState()
        state.enter("a", at: 0)
        state.enter("b", at: 0.2)
        state.advance(to: 0.4)
        #expect(state.visibleID == nil)
        state.advance(to: 0.601)
        #expect(state.visibleID == "b")
    }

    @Test func edgePlacementAndReducedMotion() {
        let window = CGSize(width: 800, height: 600), size = CGSize(width: 100, height: 30)
        #expect(HideBalloonPlacement.center(anchor: CGRect(x: 350, y: 200, width: 100, height: 20), size: size, window: window) == CGPoint(x: 400, y: 181))
        #expect(HideBalloonPlacement.center(anchor: CGRect(x: 350, y: 0, width: 100, height: 20), size: size, window: window) == CGPoint(x: 400, y: 39))
        let right = HideBalloonPlacement.center(anchor: CGRect(x: 790, y: 200, width: 10, height: 20), size: size, window: window)
        #expect(right.x + size.width / 2 == 792)
        let left = HideBalloonPlacement.center(anchor: CGRect(x: 0, y: 200, width: 10, height: 20), size: size, window: window)
        #expect(left.x - size.width / 2 == 8)
        #expect(HideTooltipState.fadeDuration(reduceMotion: true) == 0)
        #expect(HideTooltipState.fadeDuration(reduceMotion: false) == 0.120)
    }

    @Test func allCommandsUseTheCurrentRegistryDisplay() {
        let bindings: [PaneCommand: PaneShortcut] = [.closePane: PaneShortcut(key: "x", modifiers: [.control, .option])]
        let commands = ShellMenuCommand.allCases.map(HideCommand.menu) + PaneCommand.allCases.map(HideCommand.pane) + (1...9).map(HideCommand.tab) + (1...9).map(HideCommand.agent)
        for command in commands {
            let shortcut = command.shortcut(bindings: bindings)!
            #expect(command.displayString(bindings: bindings) == shortcut.displayString)
            #expect(command.tooltipText(label: "Action", bindings: bindings) == "Action (\(shortcut.displayString))")
        }
        #expect(HideCommand.label("Copy endpoint").tooltipText(label: "Copy endpoint", bindings: bindings) == "Copy endpoint")
    }
}
