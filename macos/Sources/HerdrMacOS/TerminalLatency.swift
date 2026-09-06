import AppKit
import QuartzCore
import os.signpost

/// Measures work inside the application. Input ends after the terminal writer
/// flushes bytes to the control transport; receive begins when a registered view receives bytes.
/// Neither interval includes the automation driver or the remote shell's echo.
@MainActor
enum TerminalLatency {
    enum Interval: String, CaseIterable {
        case keyToSend = "key_to_send"
        case receiveToDraw = "receive_to_draw"
        case wheelToDraw = "wheel_to_draw"
        case tabToDraw = "tab_to_first_draw"
    }

    private struct Pending {
        let id: OSSignpostID
        let start: CFTimeInterval
        let startedNS: UInt64
    }

    private static let log = OSLog(subsystem: "me.grab.hide", category: "TerminalLatency")
    private static var pending: [String: [Interval: [Pending]]] = [:]
    private static var inputs: [String: [UInt64: Pending]] = [:]
    private static var refreshRates: [String: Double] = [:]

    static func begin(_ interval: Interval, paneID: String) {
        guard log.signpostsEnabled || log.isEnabled(type: .debug) else { return }
        var marks = pending[paneID]?[interval] ?? []
        // An input consumed by an IME or a wheel at a boundary need not draw.
        // Keep that visible in the trace without retaining unbounded intervals.
        if marks.count == 512 {
            finish(interval, paneID: paneID, marks: marks, outcome: "capacity")
            marks.removeAll(keepingCapacity: true)
        }
        var time = timespec()
        precondition(clock_gettime(CLOCK_UPTIME_RAW, &time) == 0)
        let mark = Pending(id: OSSignpostID(log: log), start: CACurrentMediaTime(),
                           startedNS: UInt64(time.tv_sec) * 1_000_000_000 + UInt64(time.tv_nsec))
        os_signpost(.begin, log: log, name: "TerminalLatency", signpostID: mark.id,
                    "interval=%{public}s pane=%{public}s", interval.rawValue, paneID)
        marks.append(mark)
        pending[paneID, default: [:]][interval] = marks
    }

    static func takeInput(paneID: String) -> [String: Any]? {
        guard let marks = pending[paneID]?.removeValue(forKey: .keyToSend), let mark = marks.last else { return nil }
        if marks.count > 1 {
            finish(.keyToSend, paneID: paneID, marks: Array(marks.dropLast()), outcome: "consumed")
        }
        if inputs[paneID, default: [:]].count >= 512 {
            let dropped = Array(inputs.removeValue(forKey: paneID)!.values)
            finish(.keyToSend, paneID: paneID, marks: dropped, outcome: "capacity")
        }
        inputs[paneID, default: [:]][mark.id.rawValue] = mark
        return ["id": mark.id.rawValue, "started_ns": mark.startedNS]
    }

    static func inputSent(_ sent: CoreTerminalInputSent, paneID: String) {
        guard let mark = inputs[paneID]?.removeValue(forKey: sent.id) else { return }
        finish(.keyToSend, paneID: paneID, marks: [mark], outcome: sent.outcome, measured: sent.milliseconds)
    }

    static func end(_ interval: Interval, paneID: String, outcome: String = "completed") {
        guard let marks = pending[paneID]?.removeValue(forKey: interval) else { return }
        finish(interval, paneID: paneID, marks: marks, outcome: outcome)
        if pending[paneID]?.isEmpty == true { pending.removeValue(forKey: paneID) }
    }

    private static func finish(_ interval: Interval, paneID: String, marks: [Pending], outcome: String, measured: Double? = nil) {
        let now = CACurrentMediaTime()
        for mark in marks {
            os_signpost(.end, log: log, name: "TerminalLatency", signpostID: mark.id,
                        "hide_latency interval=%{public}s pane=%{public}s milliseconds=%{public}.6f hz=%{public}.3f outcome=%{public}s",
                        interval.rawValue, paneID, measured ?? (now - mark.start) * 1_000,
                        refreshRates[paneID] ?? 0, outcome)
            // The same end record is available to log stream without Instruments.
            // Debug logging is disabled unless a collector requests it.
            os_log(.debug, log: log,
                   "hide_latency interval=%{public}s pane=%{public}s milliseconds=%{public}.6f hz=%{public}.3f outcome=%{public}s",
                   interval.rawValue, paneID, measured ?? (now - mark.start) * 1_000,
                   refreshRates[paneID] ?? 0, outcome)
        }
    }

    static func drawn(paneID: String) {
        end(.receiveToDraw, paneID: paneID)
        end(.wheelToDraw, paneID: paneID)
        end(.tabToDraw, paneID: paneID)
    }

    static func hidden(paneID: String) {
        for interval in [Interval.receiveToDraw, .wheelToDraw, .tabToDraw] {
            end(interval, paneID: paneID, outcome: "hidden")
        }
    }

    static func release(paneID: String) {
        if let marks = inputs.removeValue(forKey: paneID) {
            finish(.keyToSend, paneID: paneID, marks: Array(marks.values), outcome: "released")
        }
        for interval in Interval.allCases { end(interval, paneID: paneID, outcome: "released") }
        refreshRates.removeValue(forKey: paneID)
    }

    static func displayPeriod(_ seconds: Double, paneID: String) {
        guard seconds > 0 else { return }
        refreshRates[paneID] = 1 / seconds
    }
}
