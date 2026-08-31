import AppKit
import CHerdrCore
import Foundation
import SpikeComposition
import SwiftTerm
import SwiftUI

private let schemaVersion = 1
private let callbackTarget = "spike-callback-burst"
private let remoteTarget = "mini-spike"

private let coreChangeCallback: @convention(c) (UnsafeMutableRawPointer?) -> Void = { context in
    guard let context else { return }
    Unmanaged<CoreBridge>.fromOpaque(context).takeUnretainedValue().receiveCoreChange()
}

private struct TerminalChunk: Equatable {
    let sequence: UInt64
    let bytes: [UInt8]
}

final class CoreBridge: ObservableObject, @unchecked Sendable {
    @Published private(set) var callbackSummary = "Waiting for Rust callback burst"
    @Published private(set) var callbackPassed = false
    @Published private(set) var remoteSummary = "Remote SSH TUI not started"
    @Published private(set) var remoteReady = false
    @Published private(set) var delegateSummary = "SwiftTerm delegate has sent 0 bytes"
    @Published private(set) var delegateBytesSent: UInt64 = 0
    @Published private(set) var schemaSummary = "Waiting for snapshot"
    @Published private(set) var schemaPassed = false
    @Published private(set) var lastError = "None"
    @Published private(set) var inputDiagnosticSummary = "Waiting for native responder state"
    @Published private(set) var inputReady = false

    var onTerminalChunk: (([UInt8]) -> Void)?

    private var core: OpaquePointer?
    private let callbackLock = NSLock()
    private var callbackObserved = 0
    private var callbacksObservedOffMain = 0
    private var callbackApplied = 0
    private var callbackObservedBaseline = 0
    private var callbackAppliedBaseline = 0
    private var callbackGateObserved: Int?
    private var callbackGateApplied: Int?
    private var callbackGateOffMain: Int?
    private var didStartBurst = false
    private var didStartRemote = false
    private var lastTerminalSequence: UInt64 = 0
    private var terminalTranscript = Data()
    private var latestSnapshot: [String: Any]?
    private var windowIsKey = false
    private var firstResponderClass = "nil"
    private var firstResponderIdentity = "nil"
    private var terminalIdentity = "nil"
    private var firstResponderIsTerminal = false
    private var terminalAcceptsFirstResponder = false
    private var terminalBecomeFirstResponderResult: Bool?
    private var appKeyDownEvents: UInt64 = 0
    private var terminalKeyDownEvents: UInt64 = 0
    private var delegateInputEvents: UInt64 = 0
    private var latestDelegateInputBytes: [UInt8] = []
    private let evidencePath: String?
    private let imeTracePath: String?
    private let requiredSnapshotKeys = [
        "schema_version", "navigator", "overlay", "tab", "connection", "zoomed",
        "focused", "terminal", "editor", "ime", "input_generation", "status", "spike",
    ]

    init() {
        evidencePath = Self.commandLineValue(after: "--evidence-path")
        imeTracePath = Self.commandLineValue(after: "--ime-trace-path")
        let options: [String: Any] = [
            "schema_version": schemaVersion,
            "herdr_socket_path": NSNull(),
            "remote_targets": [[
                "id": remoteTarget,
                "label": "Isolated mini SSH fixture",
                "ssh_alias": "mini",
            ]],
            "app_state_path": "/tmp/herdr-ide-verify-swift-shell-state.json",
        ]
        guard
            let data = try? JSONSerialization.data(withJSONObject: options),
            let created = data.withUnsafeBytes({ rawBuffer in
                herdr_core_create(rawBuffer.bindMemory(to: UInt8.self).baseAddress, data.count)
            })
        else {
            lastError = "herdr_core_create returned null"
            return
        }
        core = created
        herdr_core_on_change(
            created,
            coreChangeCallback,
            Unmanaged.passUnretained(self).toOpaque()
        )
        refreshSnapshot()
        DispatchQueue.main.async { [weak self] in
            self?.startCallbackBurst()
        }
    }

    #if SPIKE_BOUNDARY_PROBE
    var delegateInputEventsForProbe: UInt64 { delegateInputEvents }
    var latestDelegateInputBytesForProbe: [UInt8] { latestDelegateInputBytes }
    #endif

    deinit {
        if let core {
            herdr_core_on_change(core, nil, nil)
            herdr_core_destroy(core)
        }
    }

    nonisolated func receiveCoreChange() {
        callbackLock.lock()
        callbackObserved += 1
        if !Thread.isMainThread {
            callbacksObservedOffMain += 1
        }
        callbackLock.unlock()

        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.callbackApplied += 1
            self.refreshSnapshot()
        }
    }

    func sendTerminal(_ bytes: [UInt8]) {
        dispatch(kind: "key", payload: [
            "pane_id": "remote-tui-fixture",
            "bytes_base64": Data(bytes).base64EncodedString(),
        ])
    }

    func recordResponderState(
        terminal: TerminalView,
        becomeFirstResponderResult: Bool? = nil
    ) {
        precondition(Thread.isMainThread)
        let responder = terminal.window?.firstResponder
        windowIsKey = terminal.window?.isKeyWindow == true
        firstResponderClass = responder.map { String(describing: type(of: $0)) } ?? "nil"
        firstResponderIdentity = responder.map { String(describing: ObjectIdentifier($0)) } ?? "nil"
        terminalIdentity = String(describing: ObjectIdentifier(terminal))
        firstResponderIsTerminal = responder === terminal
        terminalAcceptsFirstResponder = terminal.acceptsFirstResponder
        if let becomeFirstResponderResult {
            terminalBecomeFirstResponderResult = becomeFirstResponderResult
        }
        inputReady = windowIsKey && firstResponderIsTerminal && terminalAcceptsFirstResponder
        inputDiagnosticSummary = "key=\(windowIsKey), first=\(firstResponderClass), terminal=\(firstResponderIsTerminal), app/terminal/delegate key events=\(appKeyDownEvents)/\(terminalKeyDownEvents)/\(delegateInputEvents)"
        writeEvidence(snapshot: latestSnapshot)
    }

    func recordTerminalKeyDown(terminal: TerminalView) {
        precondition(Thread.isMainThread)
        terminalKeyDownEvents += 1
        recordResponderState(terminal: terminal)
    }

    func recordDelegateInput(terminal: TerminalView, bytes: [UInt8]) {
        precondition(Thread.isMainThread)
        delegateInputEvents += 1
        latestDelegateInputBytes = bytes
        recordResponderState(terminal: terminal)
    }

    private func startCallbackBurst() {
        guard !didStartBurst else { return }
        didStartBurst = true
        callbackLock.lock()
        callbackObservedBaseline = callbackObserved
        callbackLock.unlock()
        callbackAppliedBaseline = callbackApplied
        dispatch(kind: "retry_connect", payload: ["target_id": callbackTarget])
    }

    private func startRemoteTUI() {
        guard !didStartRemote else { return }
        didStartRemote = true
        remoteSummary = "Opening isolated SSH PTY through Rust"
        dispatch(kind: "retry_connect", payload: ["target_id": remoteTarget])
    }

    private func dispatch(kind: String, payload: [String: Any]) {
        guard let core else { return }
        let event: [String: Any] = [
            "schema_version": schemaVersion,
            "kind": kind,
            "payload": payload,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: event) else {
            lastError = "Failed to encode \(kind) event"
            return
        }
        data.withUnsafeBytes { rawBuffer in
            herdr_core_dispatch(core, rawBuffer.bindMemory(to: UInt8.self).baseAddress, data.count)
        }
    }

    private func refreshSnapshot() {
        guard let core else { return }
        let owned = herdr_core_snapshot(core)
        defer { herdr_core_free_bytes(owned) }
        guard let pointer = owned.ptr, owned.len > 0 else {
            lastError = "herdr_core_snapshot returned empty bytes"
            writeEvidence(snapshot: nil)
            return
        }
        let data = Data(bytes: pointer, count: owned.len)
        guard let snapshot = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            lastError = "Snapshot JSON could not be decoded"
            writeEvidence(snapshot: nil)
            return
        }
        latestSnapshot = snapshot

        let keysPresent = requiredSnapshotKeys.allSatisfy { snapshot.keys.contains($0) }
        let version = snapshot["schema_version"] as? Int
        schemaPassed = keysPresent && version == schemaVersion
        schemaSummary = schemaPassed
            ? "Schema v\(schemaVersion), all \(requiredSnapshotKeys.count) top-level fields present"
            : "Snapshot schema is incomplete or version-mismatched"

        callbackLock.lock()
        let observed = callbackObserved - callbackObservedBaseline
        let offMain = callbacksObservedOffMain
        callbackLock.unlock()
        let applied = callbackApplied - callbackAppliedBaseline
        let emitted = ((snapshot["spike"] as? [String: Any])?["callback_emitted"] as? NSNumber)?.intValue ?? 0
        if callbackGateObserved == nil,
           emitted == 100,
           observed == 100,
           applied == 100,
           offMain == 100 {
            callbackGateObserved = observed
            callbackGateApplied = applied
            callbackGateOffMain = offMain
        }
        callbackPassed = callbackGateObserved == 100
            && callbackGateApplied == 100
            && callbackGateOffMain == 100
        if callbackPassed {
            callbackSummary = "Rust emitted 100/100, Swift observed 100, main-thread applied 100, off-main received 100"
        } else {
            callbackSummary = "Rust emitted \(emitted)/100, Swift observed \(observed), main-thread applied \(applied), off-main received \(offMain)"
        }

        if callbackPassed {
            startRemoteTUI()
        }

        let spike = snapshot["spike"] as? [String: Any]
        remoteReady = (spike?["remote_tui_ready"] as? Bool) == true
        delegateBytesSent = (spike?["delegate_bytes_sent"] as? NSNumber)?.uint64Value ?? 0
        remoteSummary = remoteReady
            ? "REMOTE_TUI_READY received through Rust SSH bytes"
            : (didStartRemote ? "Waiting for remote SSH bytes" : "Remote SSH TUI not started")
        delegateSummary = "SwiftTerm delegate has sent \(delegateBytesSent) bytes to Rust"

        let status = snapshot["status"] as? [String: Any]
        if let error = status?["last_error"] as? [String: Any] {
            let kind = error["kind"] as? String ?? "unknown"
            let message = error["message"] as? String ?? "unknown"
            lastError = "\(kind): \(message)"
        } else {
            lastError = "None"
        }

        if let terminal = snapshot["terminal"] as? [String: Any],
           let chunks = terminal["chunks"] as? [[String: Any]] {
            let pending: [TerminalChunk] = chunks.compactMap { value in
                guard
                    let sequence = (value["sequence"] as? NSNumber)?.uint64Value,
                    sequence > lastTerminalSequence,
                    let encoded = value["bytes_base64"] as? String,
                    let bytes = Data(base64Encoded: encoded)
                else { return nil }
                return TerminalChunk(sequence: sequence, bytes: [UInt8](bytes))
            }.sorted { $0.sequence < $1.sequence }
            for chunk in pending {
                lastTerminalSequence = chunk.sequence
                terminalTranscript.append(contentsOf: chunk.bytes)
                onTerminalChunk?(chunk.bytes)
            }
        }

        writeEvidence(snapshot: snapshot)
    }

    private func writeEvidence(snapshot: [String: Any]?) {
        guard let evidencePath else { return }
        callbackLock.lock()
        let observed = callbackObserved - callbackObservedBaseline
        let offMain = callbacksObservedOffMain
        callbackLock.unlock()
        let applied = callbackApplied - callbackAppliedBaseline
        let transcript = String(decoding: terminalTranscript, as: UTF8.self)
        let report: [String: Any] = [
            "schema_version": schemaVersion,
            "snapshot_schema_passed": schemaPassed,
            "required_snapshot_keys": requiredSnapshotKeys,
            "callback_passed": callbackPassed,
            "callback_gate_observed": callbackGateObserved ?? 0,
            "callback_gate_applied_on_main": callbackGateApplied ?? 0,
            "callback_gate_received_off_main": callbackGateOffMain ?? 0,
            "callbacks_observed": observed,
            "callbacks_applied_on_main": applied,
            "callbacks_received_off_main": offMain,
            "remote_tui_ready": remoteReady,
            "swiftterm_delegate_bytes_sent": delegateBytesSent,
            "transcript_contains_remote_ready": transcript.contains("REMOTE_TUI_READY"),
            "transcript_contains_ascii_probe": transcript.contains("INPUT_UTF8:t2-probe"),
            "transcript_contains_hangul": transcript.contains("한"),
            "transcript_contains_immediate_echo_probe": transcript.contains("V9_ECHO_PROBE"),
            "last_error": lastError,
            "snapshot_available": snapshot != nil,
            "window_is_key": windowIsKey,
            "first_responder_class": firstResponderClass,
            "first_responder_identity": firstResponderIdentity,
            "terminal_identity": terminalIdentity,
            "first_responder_is_terminal": firstResponderIsTerminal,
            "terminal_accepts_first_responder": terminalAcceptsFirstResponder,
            "terminal_become_first_responder_result": terminalBecomeFirstResponderResult ?? false,
            "app_key_down_monitor_installed": false,
            "terminal_input_observation": "DiagnosticTerminalView.interpretKeyEvents only",
            "composition_adapter": "document-coordinate",
            "app_key_down_events": appKeyDownEvents,
            "terminal_key_down_events": terminalKeyDownEvents,
            "swiftterm_delegate_events": delegateInputEvents,
            "ime_trace_path": imeTracePath ?? "disabled",
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]) else {
            return
        }
        try? data.write(to: URL(fileURLWithPath: evidencePath), options: .atomic)
    }

    private static func commandLineValue(after flag: String) -> String? {
        guard let index = CommandLine.arguments.firstIndex(of: flag) else { return nil }
        let valueIndex = CommandLine.arguments.index(after: index)
        guard valueIndex < CommandLine.arguments.endIndex else { return nil }
        return CommandLine.arguments[valueIndex]
    }
}

final class DiagnosticTerminalView: TerminalView {
    weak var diagnosticsBridge: CoreBridge?
    var imeCallTrace: ImeCallTrace?
    private var compositionCoordinates = CompositionDocumentState()

    override func setMarkedText(
        _ string: Any,
        selectedRange: NSRange,
        replacementRange: NSRange
    ) {
        let baseSelection = super.selectedRange()
        let callID = imeCallTrace?.begin(
            method: "setMarkedText",
            terminal: self,
            arguments: [
                "value": ImeCallTrace.textArgument(string, revealScalars: true),
                "selected_range": ImeCallTrace.range(selectedRange),
                "replacement_range": ImeCallTrace.range(replacementRange),
            ]
        )
        let transition = compositionCoordinates.receiveMarkedText(
            utf16Length: markedTextLength(string),
            selectedRange: selectedRange,
            replacementRange: replacementRange,
            baseSelection: baseSelection
        )
        if !transition.succeeded {
            _ = compositionCoordinates.clear(operation: "invalid-marked-text-reset")
        }
        super.setMarkedText(
            string,
            selectedRange: selectedRange,
            replacementRange: replacementRange
        )
        if let callID {
            imeCallTrace?.end(
                method: "setMarkedText",
                callID: callID,
                terminal: self,
                arguments: compositionTransitionArgument(transition)
            )
        }
    }

    override func insertText(_ string: Any, replacementRange: NSRange) {
        let fallbackSelection = selectedRange()
        let resolvedDocumentRange = compositionCoordinates.resolveReplacementRange(
            replacementRange,
            fallbackSelection: fallbackSelection
        )
        let localReplacementRange = resolvedDocumentRange.flatMap {
            compositionCoordinates.translateReplacementRangeToMarkedStorage($0)
        }
        let callID = imeCallTrace?.begin(
            method: "insertText",
            terminal: self,
            arguments: [
                "value": ImeCallTrace.textArgument(string, revealScalars: hasMarkedText()),
                "replacement_range": ImeCallTrace.range(replacementRange),
                "resolved_document_replacement_range": resolvedDocumentRange.map(ImeCallTrace.range) ?? ["status": "unresolved"],
                "marked_storage_replacement_range": localReplacementRange.map(ImeCallTrace.range) ?? ["status": "not-in-active-mark"],
            ]
        )
        let transition = compositionCoordinates.clear(operation: "insert-text-commit")
        super.insertText(string, replacementRange: replacementRange)
        if let callID {
            imeCallTrace?.end(
                method: "insertText",
                callID: callID,
                terminal: self,
                arguments: compositionTransitionArgument(transition)
            )
        }
    }

    override func unmarkText() {
        let callID = imeCallTrace?.begin(method: "unmarkText", terminal: self)
        let transition = compositionCoordinates.clear(operation: "unmark-text")
        super.unmarkText()
        if let callID {
            imeCallTrace?.end(
                method: "unmarkText",
                callID: callID,
                terminal: self,
                arguments: compositionTransitionArgument(transition)
            )
        }
    }

    override func selectedRange() -> NSRange {
        compositionCoordinates.snapshot.selectedRange ?? super.selectedRange()
    }

    override func markedRange() -> NSRange {
        let range = compositionCoordinates.snapshot.markedRange
        return range.location == NSNotFound ? super.markedRange() : range
    }

    override func hasMarkedText() -> Bool {
        compositionCoordinates.snapshot.isActive || super.hasMarkedText()
    }

    override func attributedSubstring(
        forProposedRange range: NSRange,
        actualRange: NSRangePointer?
    ) -> NSAttributedString? {
        guard compositionCoordinates.snapshot.isActive else {
            return super.attributedSubstring(forProposedRange: range, actualRange: actualRange)
        }
        guard let translation = compositionCoordinates.translateDocumentRange(range) else {
            return nil
        }

        var localActualRange = NSRange(location: NSNotFound, length: 0)
        let value = super.attributedSubstring(
            forProposedRange: translation.localRange,
            actualRange: &localActualRange
        )
        if localActualRange.location != NSNotFound,
           let anchor = compositionCoordinates.snapshot.anchor,
           localActualRange.location <= Int.max - anchor {
            actualRange?.pointee = NSRange(
                location: anchor + localActualRange.location,
                length: localActualRange.length
            )
        } else if value != nil {
            actualRange?.pointee = translation.documentRange
        }
        return value
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.diagnosticsBridge?.recordResponderState(terminal: self)
        }
    }

    override func mouseDown(with event: NSEvent) {
        super.mouseDown(with: event)
        diagnosticsBridge?.recordResponderState(terminal: self)
    }

    override func interpretKeyEvents(_ eventArray: [NSEvent]) {
        diagnosticsBridge?.recordTerminalKeyDown(terminal: self)
        let callID = imeCallTrace?.begin(
            method: "interpretKeyEvents",
            terminal: self,
            arguments: [
                "event_count": eventArray.count,
                "events": eventArray.map(ImeCallTrace.keyEvent),
            ]
        )
        defer {
            if let callID {
                imeCallTrace?.end(
                    method: "interpretKeyEvents",
                    callID: callID,
                    terminal: self
                )
            }
        }
        super.interpretKeyEvents(eventArray)
    }

    func recordDelegateSend(_ data: ArraySlice<UInt8>) {
        imeCallTrace?.point(
            method: "terminalDelegate.send",
            terminal: self,
            arguments: ImeCallTrace.byteArgument(data)
        )
    }

    private func markedTextLength(_ value: Any) -> Int {
        switch value {
        case let attributed as NSAttributedString:
            return attributed.length
        case let plain as String:
            return (plain as NSString).length
        case let legacy as NSString:
            return legacy.length
        default:
            return 0
        }
    }

    private func compositionTransitionArgument(
        _ transition: CompositionDocumentState.Transition
    ) -> [String: Any] {
        var report: [String: Any] = [
            "composition_transition": transition.operation,
            "composition_transition_succeeded": transition.succeeded,
            "composition_before": compositionSnapshotArgument(transition.before),
            "composition_after": compositionSnapshotArgument(transition.after),
            "composition_current": compositionSnapshotArgument(compositionCoordinates.snapshot),
        ]
        if let failure = transition.failure {
            report["composition_transition_failure"] = failure
        }
        return report
    }

    private func compositionSnapshotArgument(
        _ snapshot: CompositionDocumentState.Snapshot
    ) -> [String: Any] {
        [
            "active": snapshot.isActive,
            "anchor": snapshot.anchor ?? "none",
            "marked_length": snapshot.markedLength,
            "relative_selection": ImeCallTrace.range(snapshot.relativeSelection),
            "document_marked_range": ImeCallTrace.range(snapshot.markedRange),
            "document_selected_range": snapshot.selectedRange.map(ImeCallTrace.range) ?? ["status": "none"],
        ]
    }
}

private struct StatusRow: View {
    let title: String
    let detail: String
    let passed: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: passed ? "checkmark.circle.fill" : "clock.fill")
                .foregroundStyle(passed ? Color.green : Color.orange)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.headline)
                Text(detail).font(.caption).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .accessibilityElement(children: .combine)
    }
}

private struct TerminalHost: NSViewRepresentable {
    @ObservedObject var bridge: CoreBridge

    func makeCoordinator() -> Coordinator {
        Coordinator(bridge: bridge)
    }

    func makeNSView(context: Context) -> TerminalView {
        let terminal = DiagnosticTerminalView(frame: .zero, font: NSFont.monospacedSystemFont(ofSize: 15, weight: .regular))
        terminal.diagnosticsBridge = bridge
        terminal.imeCallTrace = ImeCallTrace.requestedRecorder()
        terminal.terminalDelegate = context.coordinator
        terminal.nativeForegroundColor = NSColor(calibratedWhite: 0.92, alpha: 1)
        terminal.nativeBackgroundColor = NSColor(calibratedRed: 0.055, green: 0.067, blue: 0.09, alpha: 1)
        terminal.setAccessibilityIdentifier("swiftterm-terminal")
        context.coordinator.terminal = terminal
        DispatchQueue.main.async {
            context.coordinator.requestInitialFocus(for: terminal)
            #if SPIKE_BOUNDARY_PROBE
            MarkedTextBoundaryProbe.startIfRequested(terminal: terminal, bridge: bridge)
            #endif
        }
        bridge.onTerminalChunk = { [weak terminal] bytes in
            precondition(Thread.isMainThread)
            terminal?.feed(byteArray: bytes[...])
        }
        return terminal
    }

    func updateNSView(_ terminal: TerminalView, context: Context) {
        context.coordinator.bridge = bridge
        DispatchQueue.main.async {
            context.coordinator.requestInitialFocus(for: terminal)
            bridge.recordResponderState(terminal: terminal)
        }
    }

    static func dismantleNSView(_ terminal: TerminalView, coordinator: Coordinator) {
        coordinator.bridge.onTerminalChunk = nil
        terminal.terminalDelegate = nil
    }

    final class Coordinator: NSObject, TerminalViewDelegate {
        var bridge: CoreBridge
        weak var terminal: TerminalView?
        private var didRequestInitialFocus = false

        init(bridge: CoreBridge) {
            self.bridge = bridge
        }

        func requestInitialFocus(for terminal: TerminalView) {
            guard !didRequestInitialFocus,
                  let window = terminal.window,
                  window.isKeyWindow
            else { return }
            didRequestInitialFocus = true
            let result = window.makeFirstResponder(terminal)
            bridge.recordResponderState(
                terminal: terminal,
                becomeFirstResponderResult: result
            )
        }

        func send(source: TerminalView, data: ArraySlice<UInt8>) {
            precondition(Thread.isMainThread)
            (source as? DiagnosticTerminalView)?.recordDelegateSend(data)
            let bytes = Array(data)
            bridge.recordDelegateInput(terminal: source, bytes: bytes)
            bridge.sendTerminal(bytes)
        }

        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {}
        func setTerminalTitle(source: TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
        func scrolled(source: TerminalView, position: Double) {}
        func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
    }
}

private struct ContentView: View {
    @ObservedObject var bridge: CoreBridge

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Swift shell pivot - Stage 0")
                        .font(.system(size: 24, weight: .bold, design: .rounded))
                    Text("Isolated Rust FFI, SwiftTerm, remote SSH, and package proof")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Text("SPIKE ONLY")
                    .font(.caption.bold())
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(Color.orange.opacity(0.16), in: Capsule())
            }
            .padding(20)

            Divider()

            HStack(alignment: .top, spacing: 0) {
                VStack(alignment: .leading, spacing: 18) {
                    StatusRow(title: "T1 - C ABI and snapshot", detail: bridge.schemaSummary, passed: bridge.schemaPassed)
                    StatusRow(title: "T1 - Rust callback", detail: bridge.callbackSummary, passed: bridge.callbackPassed)
                    StatusRow(title: "T2/T3 - Remote SSH bytes", detail: bridge.remoteSummary, passed: bridge.remoteReady)
                    StatusRow(title: "T2 - SwiftTerm delegate", detail: bridge.delegateSummary, passed: bridge.delegateBytesSent > 0)
                    StatusRow(title: "T3 - Native input routing", detail: bridge.inputDiagnosticSummary, passed: bridge.inputReady)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Observable failure").font(.headline)
                        Text(bridge.lastError)
                            .font(.caption.monospaced())
                            .foregroundStyle(bridge.lastError == "None" ? Color.secondary : Color.red)
                            .textSelection(.enabled)
                    }
                    Spacer()
                    Text("Click the terminal, then type. Input flows SwiftTerm delegate -> Rust -> SSH PTY -> SwiftTerm feed.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(width: 310)
                .padding(20)

                Divider()

                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Text("Remote agent TUI fixture").font(.headline)
                        Spacer()
                        Text("ssh mini")
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                    }
                    TerminalHost(bridge: bridge)
                        .accessibilityLabel("SwiftTerm remote terminal")
                        .background(Color.black)
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                }
                .padding(20)
            }
        }
        .frame(minWidth: 980, minHeight: 680)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}

@main
struct SwiftShellSpikeApp: App {
    @StateObject private var bridge = CoreBridge()

    var body: some Scene {
        WindowGroup {
            ContentView(bridge: bridge)
        }
        .defaultSize(width: 1040, height: 720)
    }
}
