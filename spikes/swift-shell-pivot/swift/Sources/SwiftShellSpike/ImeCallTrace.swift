import AppKit
import Foundation
import SwiftTerm

final class ImeCallTrace: @unchecked Sendable {
    private static let traceFlag = "--ime-trace-path"
    private static let swiftTermVersion = "1.20.0"
    private static let swiftTermRevision = "5d14406844143538cd8f8851d2d8a67c1fe443e5"
    private static let eventCapacity = 512
    private static let flushDelay = DispatchTimeInterval.milliseconds(120)

    private struct Storage {
        var nextSequence: UInt64 = 1
        var nextCallID: UInt64 = 1
        var totalEventCount: UInt64 = 0
        var events: [[String: Any]] = []
        var overflowCount: UInt64 = 0
        var reentrancyDropCount: UInt64 = 0
        var writeFailureCount: UInt64 = 0
        var lastWriteError: String?
        var generation: UInt64 = 1
        var lastSuccessfulGeneration: UInt64 = 0
        var shutdownObserved = false
    }

    private let outputURL: URL
    private let startedAt = ISO8601DateFormatter().string(from: Date())
    private let storageLock = NSLock()
    private let flushQueue = DispatchQueue(label: "dev.herdr.swift-shell-spike.ime-trace-flush")
    private var storage = Storage()
    private var isCapturing = false
    private var terminationObserver: NSObjectProtocol?

    static func requestedRecorder() -> ImeCallTrace? {
        guard let path = commandLineValue(after: traceFlag) else { return nil }
        return ImeCallTrace(outputURL: URL(fileURLWithPath: path))
    }

    private init(outputURL: URL) {
        self.outputURL = outputURL
        terminationObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.willTerminateNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.flushForShutdown()
        }
        scheduleFlush(reason: "startup", delay: .milliseconds(0))
    }

    deinit {
        if let terminationObserver {
            NotificationCenter.default.removeObserver(terminationObserver)
        }
    }

    func begin(
        method: String,
        terminal: TerminalView,
        arguments: [String: Any] = [:]
    ) -> UInt64 {
        storageLock.lock()
        let callID = storage.nextCallID
        storage.nextCallID += 1
        storageLock.unlock()
        append(
            method: method,
            phase: "begin",
            callID: callID,
            terminal: terminal,
            arguments: arguments
        )
        return callID
    }

    func end(
        method: String,
        callID: UInt64,
        terminal: TerminalView,
        arguments: [String: Any] = [:]
    ) {
        append(
            method: method,
            phase: "end",
            callID: callID,
            terminal: terminal,
            arguments: arguments
        )
    }

    func point(
        method: String,
        terminal: TerminalView,
        arguments: [String: Any] = [:]
    ) {
        append(
            method: method,
            phase: "point",
            callID: nil,
            terminal: terminal,
            arguments: arguments
        )
    }

    static func textArgument(
        _ value: Any,
        revealScalars: Bool
    ) -> [String: Any] {
        let typeName = String(describing: type(of: value))
        let text: String?
        switch value {
        case let attributed as NSAttributedString:
            text = attributed.string
        case let plain as String:
            text = plain
        case let legacy as NSString:
            text = legacy as String
        default:
            text = nil
        }

        guard let text else {
            return [
                "value_type": typeName,
                "content_policy": "unsupported-value-redacted",
            ]
        }

        let containsNonASCII = text.unicodeScalars.contains { $0.value > 0x7f }
        var report: [String: Any] = [
            "value_type": typeName,
            "utf16_length": text.utf16.count,
            "unicode_scalar_count": text.unicodeScalars.count,
            "contains_non_ascii": containsNonASCII,
        ]
        if revealScalars || containsNonASCII {
            report["content_policy"] = "unicode-scalars-for-ime-diagnosis"
            report["unicode_scalars_hex"] = text.unicodeScalars.map {
                String(format: "U+%04X", $0.value)
            }
        } else {
            report["content_policy"] = "printable-content-redacted"
        }
        return report
    }

    static func range(_ range: NSRange) -> [String: Any] {
        [
            "location": range.location == NSNotFound ? "NSNotFound" : range.location,
            "length": range.length,
        ]
    }

    static func keyEvent(_ event: NSEvent) -> [String: Any] {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var names: [String] = []
        if modifiers.contains(.command) { names.append("command") }
        if modifiers.contains(.control) { names.append("control") }
        if modifiers.contains(.option) { names.append("option") }
        if modifiers.contains(.shift) { names.append("shift") }
        if modifiers.contains(.function) { names.append("function") }
        if modifiers.contains(.capsLock) { names.append("capsLock") }
        return [
            "key_code": event.keyCode,
            "modifiers": names,
            "is_repeat": event.isARepeat,
            "characters_redacted": true,
        ]
    }

    static func byteArgument(_ bytes: ArraySlice<UInt8>) -> [String: Any] {
        let values = Array(bytes)
        let isDiagnosticControl = values.allSatisfy { $0 < 0x20 || $0 == 0x7f }
        let containsNonASCII = values.contains { $0 >= 0x80 }
        var report: [String: Any] = [
            "byte_count": values.count,
            "contains_non_ascii": containsNonASCII,
        ]
        if isDiagnosticControl || containsNonASCII {
            report["content_policy"] = "hex-for-input-path-diagnosis"
            report["bytes_hex"] = values.map { String(format: "%02X", $0) }
        } else {
            report["content_policy"] = "printable-content-redacted"
        }
        return report
    }

    private func append(
        method: String,
        phase: String,
        callID: UInt64?,
        terminal: TerminalView,
        arguments: [String: Any]
    ) {
        precondition(Thread.isMainThread)
        guard !isCapturing else {
            storageLock.lock()
            storage.reentrancyDropCount += 1
            storage.generation += 1
            storageLock.unlock()
            scheduleFlush(reason: "reentrancy-drop")
            return
        }
        isCapturing = true
        defer { isCapturing = false }

        let terminalState = state(of: terminal)
        storageLock.lock()
        let sequence = storage.nextSequence
        storage.nextSequence += 1
        storage.totalEventCount += 1
        var event: [String: Any] = [
            "sequence": sequence,
            "uptime_nanoseconds": DispatchTime.now().uptimeNanoseconds,
            "method": method,
            "phase": phase,
            "arguments": arguments,
            "state": terminalState,
        ]
        if let callID {
            event["call_id"] = callID
        }
        if storage.events.count == Self.eventCapacity {
            storage.events.removeFirst()
            storage.overflowCount += 1
        }
        storage.events.append(event)
        storage.generation += 1
        storageLock.unlock()
        scheduleFlush(reason: "coalesced-event")
    }

    private func state(of terminal: TerminalView) -> [String: Any] {
        let markedRange = terminal.markedRange()
        let selectedRange = terminal.selectedRange()
        var report: [String: Any] = [
            "has_marked_text": terminal.hasMarkedText(),
            "marked_range": Self.range(markedRange),
            "selected_range": Self.range(selectedRange),
            "window_is_key": terminal.window?.isKeyWindow == true,
            "first_responder_is_terminal": terminal.window?.firstResponder === terminal,
        ]

        guard markedRange.location != NSNotFound,
              markedRange.length > 0
        else {
            report["marked_value"] = [
                "content_policy": "none",
                "utf16_length": 0,
                "unicode_scalar_count": 0,
            ]
            return report
        }

        var actualRange = NSRange(location: NSNotFound, length: 0)
        if let marked = terminal.attributedSubstring(
            forProposedRange: markedRange,
            actualRange: &actualRange
        ) {
            report["marked_value"] = Self.textArgument(marked, revealScalars: true)
            report["marked_value_actual_range"] = Self.range(actualRange)
        } else {
            report["marked_value"] = ["content_policy": "unavailable"]
        }
        return report
    }

    private func scheduleFlush(
        reason: String,
        delay: DispatchTimeInterval = ImeCallTrace.flushDelay
    ) {
        flushQueue.asyncAfter(deadline: .now() + delay) { [weak self] in
            self?.flushLatest(reason: reason)
        }
    }

    private func flushLatest(reason: String) {
        storageLock.lock()
        let generation = storage.generation
        guard generation > storage.lastSuccessfulGeneration else {
            storageLock.unlock()
            return
        }
        let events = storage.events
        let totalEventCount = storage.totalEventCount
        let overflowCount = storage.overflowCount
        let reentrancyDropCount = storage.reentrancyDropCount
        let writeFailureCount = storage.writeFailureCount
        let previousWriteError = storage.lastWriteError
        let shutdownObserved = storage.shutdownObserved
        storageLock.unlock()

        var report: [String: Any] = [
            "schema_version": 1,
            "trace_kind": "real-appkit-nstextinputclient-call-stream",
            "started_at": startedAt,
            "swiftterm_version": Self.swiftTermVersion,
            "swiftterm_revision": Self.swiftTermRevision,
            "privacy": [
                "ordinary_printable_content": "redacted",
                "ime_marked_or_non_ascii_content": "unicode scalar values only",
                "key_events": "key code and modifiers only",
            ],
            "recorder": [
                "capacity": Self.eventCapacity,
                "retained_event_count": events.count,
                "total_event_count": totalEventCount,
                "overflow_count": overflowCount,
                "reentrancy_drop_count": reentrancyDropCount,
                "write_failure_count_before_this_flush": writeFailureCount,
                "flush_completed_generation": generation,
                "flush_reason": reason,
                "shutdown_observed": shutdownObserved,
                "liveness": shutdownObserved ? "shutdown-flushed" : "active",
            ],
            "event_count": events.count,
            "events": events,
        ]
        if let previousWriteError {
            report["previous_write_error"] = previousWriteError
        }

        do {
            let data = try JSONSerialization.data(
                withJSONObject: report,
                options: [.prettyPrinted, .sortedKeys]
            )
            try data.write(to: outputURL, options: .atomic)
            storageLock.lock()
            storage.lastSuccessfulGeneration = max(storage.lastSuccessfulGeneration, generation)
            storage.lastWriteError = nil
            storageLock.unlock()
        } catch {
            storageLock.lock()
            storage.writeFailureCount += 1
            storage.lastWriteError = String(describing: error)
            storageLock.unlock()
            fputs("IME trace write failed: \(error)\n", stderr)
        }
    }

    private func flushForShutdown() {
        precondition(Thread.isMainThread)
        storageLock.lock()
        storage.shutdownObserved = true
        storage.generation += 1
        storageLock.unlock()
        flushQueue.sync { [self] in
            flushLatest(reason: "application-will-terminate")
        }
    }

    private static func commandLineValue(after flag: String) -> String? {
        guard let index = CommandLine.arguments.firstIndex(of: flag) else { return nil }
        let valueIndex = CommandLine.arguments.index(after: index)
        guard valueIndex < CommandLine.arguments.endIndex else { return nil }
        return CommandLine.arguments[valueIndex]
    }
}
