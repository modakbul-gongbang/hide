#if SPIKE_BOUNDARY_PROBE
import AppKit
import Foundation
import SwiftTerm

enum MarkedTextBoundaryProbe {
    private static var didStart = false

    static func startIfRequested(terminal: DiagnosticTerminalView, bridge: CoreBridge) {
        guard !didStart, let outputPath = commandLineValue(after: "--marked-text-probe") else {
            return
        }
        didStart = true
        NSApp.activate(ignoringOtherApps: true)
        terminal.window?.makeKeyAndOrderFront(nil)
        _ = terminal.window?.makeFirstResponder(terminal)
        waitUntilReady(terminal: terminal, bridge: bridge, outputPath: outputPath, attemptsRemaining: 40)
    }

    private static func waitUntilReady(
        terminal: DiagnosticTerminalView,
        bridge: CoreBridge,
        outputPath: String,
        attemptsRemaining: Int
    ) {
        let ready = bridge.remoteReady
            && terminal.window?.isKeyWindow == true
            && terminal.window?.firstResponder === terminal
        guard ready else {
            if attemptsRemaining == 0 {
                finish(
                    outputPath: outputPath,
                    report: [
                        "passed": false,
                        "failure": "terminal did not become a ready key-window first responder",
                        "remote_tui_ready": bridge.remoteReady,
                        "window_is_key": terminal.window?.isKeyWindow == true,
                        "first_responder_is_terminal": terminal.window?.firstResponder === terminal,
                    ]
                )
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                waitUntilReady(
                    terminal: terminal,
                    bridge: bridge,
                    outputPath: outputPath,
                    attemptsRemaining: attemptsRemaining - 1
                )
            }
            return
        }

        let baseSelection = terminal.selectedRange()
        terminal.setMarkedText(
            "한",
            selectedRange: NSRange(location: 1, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )
        let implicitState = markedState(of: terminal)
        terminal.setMarkedText(
            "가나",
            selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: baseSelection.location + 5, length: 0)
        )
        let explicitState = markedState(of: terminal)
        var partialActual = NSRange(location: NSNotFound, length: 0)
        let partialText = terminal.attributedSubstring(
            forProposedRange: NSRange(location: baseSelection.location + 6, length: 1),
            actualRange: &partialActual
        )?.string
        terminal.unmarkText()

        let ordinaryEventsBefore = bridge.delegateInputEventsForProbe
        let ordinaryBytesBefore = bridge.delegateBytesSent
        terminal.keyDown(with: keyEvent(keyCode: 51, characters: "\u{7f}", window: terminal.window))
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
            let ordinaryEventDelta = bridge.delegateInputEventsForProbe - ordinaryEventsBefore
            let ordinaryByteDelta = bridge.delegateBytesSent - ordinaryBytesBefore
            let lastDelegateBytes = bridge.latestDelegateInputBytesForProbe
            let expectedImplicitMarkedRange = NSRange(location: baseSelection.location, length: 1)
            let expectedImplicitSelectedRange = NSRange(location: baseSelection.location + 1, length: 0)
            let expectedExplicitMarkedRange = NSRange(location: baseSelection.location + 5, length: 2)
            let expectedExplicitSelectedRange = NSRange(location: baseSelection.location + 7, length: 0)
            finish(
                outputPath: outputPath,
                report: [
                    "passed": baseSelection.location != NSNotFound
                        && implicitState.hasText
                        && implicitState.text == "한"
                        && implicitState.range == expectedImplicitMarkedRange
                        && implicitState.selectedRange == expectedImplicitSelectedRange
                        && implicitState.actualRange == expectedImplicitMarkedRange
                        && explicitState.hasText
                        && explicitState.text == "가나"
                        && explicitState.range == expectedExplicitMarkedRange
                        && explicitState.selectedRange == expectedExplicitSelectedRange
                        && explicitState.actualRange == expectedExplicitMarkedRange
                        && partialText == "나"
                        && partialActual == NSRange(location: baseSelection.location + 6, length: 1)
                        && ordinaryEventDelta == 1
                        && ordinaryByteDelta == 1
                        && lastDelegateBytes == [0x7f],
                    "acceptance_scope": "document-coordinate and ordinary-backspace AppKit boundary only; not a real IME verdict",
                    "base_selection": rangeDictionary(baseSelection),
                    "window_is_key": terminal.window?.isKeyWindow == true,
                    "first_responder_is_terminal": terminal.window?.firstResponder === terminal,
                    "implicit_marked_state": implicitState.dictionary,
                    "explicit_marked_state": explicitState.dictionary,
                    "partial_substring": partialText ?? "nil",
                    "partial_actual_range": rangeDictionary(partialActual),
                    "ordinary_delegate_event_delta": ordinaryEventDelta,
                    "ordinary_rust_byte_delta": ordinaryByteDelta,
                    "ordinary_delegate_bytes": lastDelegateBytes,
                ]
            )
        }
    }

    private static func keyEvent(keyCode: UInt16, characters: String, window: NSWindow?) -> NSEvent {
        guard let event = NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window?.windowNumber ?? 0,
            context: nil,
            characters: characters,
            charactersIgnoringModifiers: characters,
            isARepeat: false,
            keyCode: keyCode
        ) else {
            preconditionFailure("failed to create key event")
        }
        return event
    }

    private static func markedState(of terminal: DiagnosticTerminalView) -> MarkedState {
        let range = terminal.markedRange()
        let selectedRange = terminal.selectedRange()
        var actual = NSRange(location: NSNotFound, length: 0)
        let text: String
        if range.location != NSNotFound,
           range.length > 0,
           let attributed = terminal.attributedSubstring(
               forProposedRange: range,
               actualRange: &actual
           ) {
            text = attributed.string
        } else {
            text = ""
        }
        return MarkedState(
            hasText: terminal.hasMarkedText(),
            range: range,
            selectedRange: selectedRange,
            actualRange: actual,
            text: text
        )
    }

    private static func rangeDictionary(_ range: NSRange) -> [String: Any] {
        [
            "location": range.location == NSNotFound ? "NSNotFound" : range.location,
            "length": range.length,
        ]
    }

    private static func finish(outputPath: String, report: [String: Any]) -> Never {
        let data = try! JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        try! data.write(to: URL(fileURLWithPath: outputPath), options: .atomic)
        fflush(nil)
        exit((report["passed"] as? Bool) == true ? EXIT_SUCCESS : EXIT_FAILURE)
    }

    private static func commandLineValue(after flag: String) -> String? {
        guard let index = CommandLine.arguments.firstIndex(of: flag) else { return nil }
        let valueIndex = CommandLine.arguments.index(after: index)
        guard valueIndex < CommandLine.arguments.endIndex else { return nil }
        return CommandLine.arguments[valueIndex]
    }
}

private struct MarkedState {
    let hasText: Bool
    let range: NSRange
    let selectedRange: NSRange
    let actualRange: NSRange
    let text: String

    var dictionary: [String: Any] {
        [
            "has_text": hasText,
            "range_location": range.location,
            "range_length": range.length,
            "selected_range_location": selectedRange.location,
            "selected_range_length": selectedRange.length,
            "actual_range_location": actualRange.location,
            "actual_range_length": actualRange.length,
            "text": text,
        ]
    }
}
#endif
