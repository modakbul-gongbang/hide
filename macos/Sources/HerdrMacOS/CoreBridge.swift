import CHerdrCore
import Foundation

private let coreSchemaVersion = 2

private let coreChangeCallback: @convention(c) (UnsafeMutableRawPointer?) -> Void = { context in
    guard let context else { return }
    let bridge = Unmanaged<CoreBridge>.fromOpaque(context).takeUnretainedValue()
    bridge.receiveCoreChange()
}

struct TerminalDelivery {
    let bytes: [UInt8]
    let frame: CoreTerminalFrame?
    let inputSent: CoreTerminalInputSent?
}

private struct RuntimeStartupPreparation: Sendable {
    let selection: HerdrRuntimeSelection?
    let environment: [String: String]
}

@MainActor
final class CoreBridge: ObservableObject, @unchecked Sendable {
    @Published private(set) var snapshot: CoreSnapshot?
    @Published private(set) var bridgeError: String?
    @Published private(set) var runtimeSelection: HerdrRuntimeSelection?

    let workspaceRoot: URL
    let isRemoteWorkspace: Bool

    nonisolated(unsafe) private var core: OpaquePointer?
    private var launchedHerdrServer: Process?
    private let statePath: String
    private let fixtureMode: Bool
    #if DEBUG
    private let verificationSnapshotPath: String?
    private let verificationSnapshotOutput: String?
    // Assigned once during initialization; Dispatch source cancellation is thread-safe.
    nonisolated(unsafe) private var verificationSnapshotWatcher: (any DispatchSourceFileSystemObject)?
    #endif
    /// Why this launch cannot proceed, when it cannot. Nil once the runtime
    /// resolved and the server started, whatever the connection does after.
    @Published private(set) var startupDiagnostic: String?
    private var runtimeInitializationStarted = false
    /// The runtime resolution started at init, awaited once before the core
    /// is created. Held so the two are the same piece of work rather than two
    /// resolutions racing.
    private var runtimePreparation: Task<RuntimeStartupPreparation, Never>?
    private var lastLoggedHerdrState: String?
    private var lastLoggedErrorKind: String?
    private var lastLoggedProjection: String?
    private var lastLoggedSnapshotRevision: UInt64?
    private var lastTerminalSequence: UInt64 = 0
    private var haveRevision: UInt64 = 0
    private var pendingTerminalBytes = PendingTerminalBuffer<TerminalDelivery>()
    private var terminalRegistrations: [String: TerminalRegistration] = [:]
    private var restoredPaneSelection = false
    private var pendingFileSave: Task<Void, Never>?
    private var pendingFileSavePayload: [String: Any]?
    private var commandDevice = CommandDevice.local
    private var routingError: String?
    var runtimeReadyHandler: (() -> Void)?
    var localHerdrMutationRejectionHandler: ((LocalHerdrMutationReadiness) -> Void)?

    var localHerdrMutationReadiness: LocalHerdrMutationReadiness {
        if fixtureMode {
            return .connected
        }
        return LocalHerdrMutationPolicy.evaluate(
            runtimeSelection: runtimeSelection,
            status: snapshot?.status.herdr,
            startupDiagnostic: startupDiagnostic,
            hideVersion: Bundle.main.object(
                forInfoDictionaryKey: "CFBundleShortVersionString"
            ) as? String
        )
    }

    private struct CommandDevice {
        let id: String
        let label: String
        let isRemote: Bool

        static let local = CommandDevice(id: "local", label: "This Mac", isRemote: false)
    }

    private struct TerminalRegistration {
        let id: UUID
        let receive: (TerminalDelivery) -> Void
        let focus: () -> Void
    }

    init(arguments: [String] = CommandLine.arguments) {
        let initStarted = Date()
        HideLaunchTrace.mark("core_bridge.init.begin")
        isRemoteWorkspace = arguments.contains("--remote-workspace")
        workspaceRoot = LaunchArguments.value("--workspace-root", in: arguments)
            .map { URL(fileURLWithPath: $0, isDirectory: true) }
            ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
        let resolvedStatePath = LaunchArguments.value("--state-path", in: arguments)
            ?? (arguments.contains("--verification-ui-fixture")
                ? "/tmp/herdr-ide-verify-ui-state.json"
                : Self.defaultStatePath())
        // The verification fixture runs without any live herdr connection;
        // every other launch talks to the local herdr socket.
        let resolvedFixtureMode = arguments.contains("--verification-ui-fixture")
        #if DEBUG
        verificationSnapshotPath = resolvedFixtureMode
            ? LaunchArguments.value("--verification-snapshot", in: arguments) : nil
        verificationSnapshotOutput = LaunchArguments.value("--verification-snapshot-output", in: arguments)
        #endif
        statePath = resolvedStatePath
        fixtureMode = resolvedFixtureMode
        runtimeSelection = nil
        if resolvedFixtureMode {
            // The verification fixture never talks to Herdr, so it has no
            // runtime to resolve and its core is ready immediately.
            guard adoptCore(herdrBinaryPath: nil) else {
                HideLaunchTrace.mark(
                    "core_bridge.init.failed",
                    detail: "core_create_null",
                    durationMilliseconds: Int(Date().timeIntervalSince(initStarted) * 1_000)
                )
                return
            }
            #if DEBUG
            seedVerificationFixture()
            if let verificationSnapshotPath {
                let descriptor = open(verificationSnapshotPath, O_EVTONLY)
                precondition(descriptor >= 0, "Cannot observe the verification snapshot")
                let watcher = DispatchSource.makeFileSystemObjectSource(fileDescriptor: descriptor, eventMask: .write, queue: .main)
                watcher.setEventHandler { [weak self] in self?.refreshSnapshot() }
                watcher.setCancelHandler { close(descriptor) }
                verificationSnapshotWatcher = watcher
                watcher.resume()
            }
            #endif
            HideLaunchTrace.mark(
                "core_bridge.init.ready",
                detail: "fixture",
                durationMilliseconds: Int(Date().timeIntervalSince(initStarted) * 1_000)
            )
            return
        }
        startupDiagnostic = HideStartupDiagnostic.initializing
        bridgeError = startupDiagnostic
        // The core is created once, and it needs the resolved Herdr binary to
        // attach a terminal at all, so resolution has to finish first.
        // Starting it here rather than at the first window means it overlaps
        // AppKit's launch instead of following it, while the subprocesses it
        // runs stay off the main thread.
        let bundlePath = Bundle.main.path(
            forResource: "herdr",
            ofType: nil,
            inDirectory: "herdr-runtime"
        )
        runtimePreparation = Task.detached(priority: .userInitiated) {
            RuntimeStartupPreparation(
                selection: HerdrRuntimeResolver.resolve(
                    bundlePath: bundlePath,
                    pin: HerdrRuntimePinLoader.pinned
                ),
                environment: HideRuntimeEnvironment.childEnvironment()
            )
        }
        HideLaunchTrace.mark(
            "core_bridge.init.ready",
            detail: "awaiting_runtime",
            durationMilliseconds: Int(Date().timeIntervalSince(initStarted) * 1_000)
        )
    }

    /// Creates the one core of this launch, once the application has made its
    /// first window visible and the runtime it needs has been resolved.
    /// Finder launches therefore cannot lose their first window to a slow
    /// shell or CLI subprocess.
    func startRuntimeInitialization() {
        guard !fixtureMode, !runtimeInitializationStarted else { return }
        runtimeInitializationStarted = true
        let socketPath = HideRuntimeEnvironment.herdrSocketPath()
        let startedAt = Date()
        HideLaunchTrace.mark("runtime_initialization.begin")
        Task { @MainActor [weak self] in
            guard let preparation = await self?.runtimePreparation?.value else { return }
            guard let self else { return }
            self.runtimePreparation = nil
            let detail = preparation.selection.map {
                "bundled_v\($0.version)"
            } ?? "no_runtime"
            HideLaunchTrace.mark(
                "runtime_initialization.resolved",
                detail: detail,
                durationMilliseconds: Int(Date().timeIntervalSince(startedAt) * 1_000)
            )

            let socketExists = FileManager.default.fileExists(atPath: socketPath)
            self.runtimeSelection = preparation.selection
            guard self.adoptCore(herdrBinaryPath: preparation.selection?.path) else {
                self.setStartupDiagnostic(
                    "Hide could not initialize its Herdr connection. Reopen the app to retry."
                )
                HideLaunchTrace.mark("runtime_initialization.failed", detail: "core_create_null")
                return
            }
            HideLaunchTrace.markOnce(
                "core_bridge.ready",
                detail: preparation.selection != nil ? "runtime_bundled" : "socket_only"
            )
            // A missing or unverified bundle is reported even when a server
            // is already running: the core can still read that socket, but
            // nothing hide starts (agents, terminals) has a binary to run.
            guard preparation.selection != nil || socketExists else {
                self.setStartupDiagnostic(HideStartupDiagnostic.runtimeUnavailable)
                HideLaunchTrace.mark("runtime_initialization.failed", detail: "runtime_unavailable")
                return
            }

            self.setStartupDiagnostic(nil)
            switch HerdrRuntimeResolver.startServerIfNeeded(
                selection: preparation.selection,
                socketPath: socketPath,
                environment: preparation.environment
            ) {
            case .notNeeded:
                HideLaunchTrace.mark("runtime_initialization.server_not_needed")
            case .started(let process):
                self.launchedHerdrServer = process
                process.terminationHandler = { [weak self] process in
                    Task { @MainActor [weak self] in
                        guard let self, self.launchedHerdrServer != nil else { return }
                        self.launchedHerdrServer = nil
                        self.setStartupDiagnostic(
                            HideStartupDiagnostic.serverExited(status: process.terminationStatus)
                        )
                        HideLaunchTrace.mark(
                            "runtime_initialization.server_exited",
                            detail: "status_\(process.terminationStatus)"
                        )
                    }
                }
                HideLaunchTrace.mark("runtime_initialization.server_started")
            case .failed(let message):
                self.setStartupDiagnostic(message)
                HideLaunchTrace.mark("runtime_initialization.server_failed", detail: "launch_error")
            }
        }
    }

    private static func createCore(
        herdrBinaryPath: String?,
        fixtureMode: Bool,
        statePath: String
    ) -> OpaquePointer? {
        let options: [String: Any] = [
            "schema_version": coreSchemaVersion,
            "herdr_socket_path": fixtureMode ? NSNull() : HideRuntimeEnvironment.herdrSocketPath() as Any,
            "herdr_bin_path": herdrBinaryPath.map { $0 as Any } ?? NSNull(),
            "app_state_path": statePath,
        ]
        guard
            let data = try? JSONSerialization.data(withJSONObject: options),
            let created = data.withUnsafeBytes({ buffer in
                herdr_core_create(buffer.bindMemory(to: UInt8.self).baseAddress, data.count)
            })
        else {
            return nil
        }
        return created
    }

    /// Creates this launch's core and takes ownership of it. A bridge holds
    /// one core for its whole life, so this runs once.
    private func adoptCore(herdrBinaryPath: String?) -> Bool {
        guard let created = Self.createCore(
            herdrBinaryPath: herdrBinaryPath,
            fixtureMode: fixtureMode,
            statePath: statePath
        ) else {
            bridgeError = "herdr_core_create returned null"
            return false
        }
        core = created
        herdr_core_on_change(
            created,
            coreChangeCallback,
            Unmanaged.passUnretained(self).toOpaque()
        )
        startupDiagnostic = nil
        refreshSnapshot()
        runtimeReadyHandler?()
        return true
    }

    private func setStartupDiagnostic(_ message: String?) {
        startupDiagnostic = message
        bridgeError = message
    }

    deinit {
        #if DEBUG
        verificationSnapshotWatcher?.cancel()
        #endif
        if let core {
            herdr_core_on_change(core, nil, nil)
            herdr_core_destroy(core)
        }
    }

    nonisolated func receiveCoreChange() {
        DispatchQueue.main.async { [weak self] in
            self?.refreshSnapshot()
        }
    }

    func sendTerminalInput(_ bytes: [UInt8], paneID: String? = nil) {
        // Input goes to the pane the terminal is attached to; the loopback
        // pane only exists for the verification fixture.
        let target = paneID ?? snapshot?.terminal.paneID ?? "local-loopback"
        var payload: [String: Any] = ["pane_id": target, "bytes_base64": Data(bytes).base64EncodedString()]
        if let trace = TerminalLatency.takeInput(paneID: target) { payload["input_trace"] = trace }
        dispatch(kind: "key", payload: payload)
    }

    func clickTerminal(paneID: String, column: Int, row: Int, modifiers: Int) {
        dispatch(kind: "terminal_click", payload: [
            "pane_id": paneID, "column": column, "row": row, "modifiers": modifiers,
        ])
    }

    @discardableResult
    func reportTerminalViewport(
        paneID: String,
        cols: Int,
        rows: Int,
        newView: Bool
    ) -> CoreDispatchOutcome {
        dispatch(kind: "terminal_viewport", payload: [
            "pane_id": paneID,
            "cols": cols,
            "rows": rows,
            "new_view": newView,
        ])
    }

    @discardableResult
    func resizeTerminal(paneID: String, cols: Int, rows: Int) -> CoreDispatchOutcome {
        dispatch(kind: "terminal_resize", payload: [
            "pane_id": paneID,
            "cols": cols,
            "rows": rows,
        ])
    }

    /// Herdr owns the pane's history, so the wheel is forwarded to it rather
    /// than moving a local buffer that the rendered stream never fills.
    func scrollTerminal(paneID: String, direction: String, lines: Int, column: Int, row: Int, modifiers: Int) {
        dispatch(kind: "terminal_scroll", payload: [
            "pane_id": paneID,
            "direction": direction,
            "lines": lines,
            "column": column, "row": row, "modifiers": modifiers,
        ])
    }

    /// Why a pane focus is being asked for.
    ///
    /// The core raises a pane's read record only for an operator focus, so a
    /// launch restore reinstating the last session's selection must say so:
    /// the operator has not looked at what changed while the app was closed.
    enum PaneFocusOrigin: String {
        case operatorChoice = "operator"
        case restore
    }

    @discardableResult
    func focusPane(
        _ paneID: String,
        origin: PaneFocusOrigin,
        requestID: String? = nil
    ) -> CoreDispatchOutcome {
        var payload: [String: Any] = ["pane_id": paneID, "origin": origin.rawValue]
        if let requestID {
            payload["request_id"] = requestID
        }
        return dispatch(kind: "focus_pane", payload: payload)
    }

    /// The core owns the ladder and its bounds, so the shell sends a direction
    /// rather than a computed size and reads the result back off the snapshot.
    var paneFind: CorePaneFindSnapshot { snapshot?.find ?? .empty }

    /// Searches a pane's whole scrollback, or steps to the next or previous
    /// match. An empty term clears the search.
    ///
    /// `step` is relative so that typing and stepping share one path: 0
    /// searches and stays put, +1 and -1 move.
    func findInPane(
        paneID: String,
        term: String,
        caseSensitive: Bool = false,
        wholeWord: Bool = false,
        regex: Bool = false,
        step: Int = 0
    ) {
        dispatch(
            kind: "pane_find",
            payload: [
                "pane_id": paneID,
                "term": term,
                "case_sensitive": caseSensitive,
                "whole_word": wholeWord,
                "regex": regex,
                "step": step,
            ]
        )
    }

    func setPaneTextScale(paneID: String, direction: PaneTextScaleDirection) {
        dispatch(
            kind: "pane_text_scale",
            payload: ["pane_id": paneID, "direction": direction.rawValue]
        )
    }

    func setEditorTextScale(direction: PaneTextScaleDirection) {
        dispatch(kind: "editor_text_scale", payload: ["direction": direction.rawValue])
    }

    var pet: CorePetSnapshot? { snapshot?.pet }

    func setPetVisible(_ visible: Bool) {
        dispatch(kind: "pet_set_visible", payload: ["visible": visible])
    }

    func togglePetVisible() {
        dispatch(kind: "pet_toggle_visible", payload: [:])
    }

    func movePet(to origin: CGPoint) {
        dispatch(kind: "pet_move", payload: ["x": origin.x, "y": origin.y])
    }

    func setPetDragging(_ dragging: Bool) {
        dispatch(kind: "pet_drag", payload: ["dragging": dragging])
    }

    func notePetActivity() {
        dispatch(kind: "pet_activity", payload: [:])
    }

    func updatePetShortcut(accelerator: String?, error: String?) {
        dispatch(kind: "pet_shortcut_update", payload: [
            "accelerator": accelerator.map { $0 as Any } ?? NSNull(),
            "error": error.map { $0 as Any } ?? NSNull(),
        ])
    }

    func focusCheckout(workspaceID: String, checkoutID: String) {
        dispatch(kind: "focus_checkout", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
        ])
    }

    func toggleInactiveCheckouts(projectPath: String) {
        dispatch(
            kind: "inactive_checkouts_toggle",
            payload: ["project_path": projectPath]
        )
    }

    func toggleInactiveProjects(deviceID: String) {
        dispatch(
            kind: "inactive_projects_toggle",
            payload: ["device_id": deviceID]
        )
    }

    func reconnectPane(_ paneID: String) {
        dispatch(kind: "reconnect_pane", payload: ["pane_id": paneID])
    }

    func focusTab(workspaceID: String, checkoutID: String, tabID: String) {
        if let paneID = snapshot?.paneLayouts.first(where: { $0.tabID == tabID })?.focusedPaneID {
            TerminalLatency.end(.tabToDraw, paneID: paneID, outcome: "superseded")
            TerminalLatency.begin(.tabToDraw, paneID: paneID)
        }
        dispatch(kind: "focus_tab", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
            "tab_id": tabID,
        ])
    }

    /// Asks the core to put one strip entry at another place in the strip.
    ///
    /// `tabID` is the strip entry's id, which spans both kinds, and `toIndex`
    /// is where it ends up in the resulting strip. The core decides whether
    /// that needs anything from Herdr; the shell only reports the drop.
    func reorderTab(workspaceID: String, checkoutID: String, tabID: String, toIndex: Int) {
        dispatch(kind: "reorder_tab", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
            "tab_id": tabID,
            "to_index": toIndex,
        ])
    }

    func focusDevice(_ deviceID: String) {
        dispatch(kind: "focus_device", payload: ["device_id": deviceID])
    }

    func listRemoteFiles(targetID: String, rootPath: String) {
        dispatch(kind: "remote_file_list", payload: [
            "target_id": targetID,
            "root_path": rootPath,
        ])
    }

    /// Synchronizes the shell's visible device selection with the local-core
    /// write boundary. This is intentionally synchronous so a shortcut pressed
    /// immediately after selecting a remote device cannot race a snapshot.
    func selectCommandDevice(id: String, label: String, isRemote: Bool) {
        commandDevice = CommandDevice(id: id, label: label, isRemote: isRemote)
        routingError = nil
        bridgeError = snapshot?.status.lastError.map { "\($0.kind): \($0.message)" }
            ?? startupDiagnostic
        HideLaunchTrace.mark(
            "core.command_device",
            detail: "id=\(id) remote=\(isRemote)"
        )
    }

    func createWorkspace(path: URL, label: String, initializeGit: Bool) {
        dispatch(kind: "create_workspace", payload: [
            "path": path.path,
            "label": label,
            "initialize_git": initializeGit,
        ])
    }

    func removeWorkspace(_ workspaceID: String) {
        dispatch(kind: "remove_workspace", payload: ["workspace_id": workspaceID])
    }

    func setWorkspacePinned(_ workspaceID: String, pinned: Bool) {
        dispatch(kind: "workspace_pin_set", payload: ["workspace_id": workspaceID, "pinned": pinned])
    }

    func registerDevice(id: String, label: String, sshAlias: String) {
        dispatch(kind: "register_device", payload: [
            "id": id,
            "label": label,
            "ssh_alias": sshAlias,
        ])
    }

    func removeDevice(_ deviceID: String) {
        dispatch(kind: "remove_device", payload: ["device_id": deviceID])
    }

    func testDevice(_ deviceID: String) {
        dispatch(kind: "test_device", payload: ["device_id": deviceID])
    }

    func createTab(workspaceID: String, checkoutID: String? = nil, label: String = "Tab 1") {
        dispatch(kind: "create_tab", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID.map { $0 as Any } ?? NSNull(),
            "label": label,
        ])
    }

    func closeTab(_ tabID: String, confirmed: Bool) {
        dispatch(kind: "close_tab", payload: [
            "tab_id": tabID,
            "confirmed": confirmed,
        ])
    }

    func reopenClosed() {
        dispatch(kind: "reopen_closed", payload: [:])
    }

    func checkCloseStatus(_ key: String) {
        dispatch(kind: "check_close_status", payload: ["key": key])
    }

    func refreshStatus() {
        dispatch(kind: "refresh_status", payload: [:])
    }

    func startAgentInCreatedPane(
        paneID: String,
        path: String,
        provider: AgentProvider,
        completion: @escaping @MainActor (AgentLaunchResult) -> Void
    ) {
        guard case .connected = localHerdrMutationReadiness else {
            completion(AgentLaunchResult(
                succeeded: false,
                message: localHerdrMutationReadiness.message,
                paneID: paneID
            ))
            return
        }
        guard let runtimeSelection else {
            completion(AgentLaunchResult(
                succeeded: false,
                message: HideStartupDiagnostic.runtimeUnavailable,
                paneID: paneID
            ))
            return
        }
        let herdrPath = runtimeSelection.path
        Task { @MainActor in
            let result = await Task.detached {
                HerdrAgentLauncher.startInPane(
                    paneID: paneID,
                    path: path,
                    provider: provider,
                    agentIsInstalled: AgentCLIAvailability.isUsable(provider.rawValue),
                    run: { arguments in
                        HerdrAgentLauncher.run(herdrPath: herdrPath, arguments: arguments)
                    }
                )
            }.value
            completion(result)
        }
    }

    @discardableResult
    func registerTerminal(
        paneID: String,
        receive: @escaping (TerminalDelivery) -> Void,
        focus: @escaping () -> Void
    ) -> UUID {
        let registrationID = UUID()
        terminalRegistrations[paneID] = TerminalRegistration(
            id: registrationID,
            receive: receive,
            focus: focus
        )
        drainPendingTerminalBytes(for: paneID)
        if snapshot?.terminal.paneID == paneID {
            DispatchQueue.main.async { [weak self] in
                guard self?.terminalRegistrations[paneID]?.id == registrationID else { return }
                self?.terminalRegistrations[paneID]?.focus()
            }
        }
        return registrationID
    }

    /// Puts one pane's terminal view in front of the keyboard. The pane's own
    /// registration owns the view, so the focus request goes through it rather
    /// than through a second reference to the same view.
    func focusTerminal(paneID: String) {
        terminalRegistrations[paneID]?.focus()
    }

    func unregisterTerminal(paneID: String, registrationID: UUID) {
        guard terminalRegistrations[paneID]?.id == registrationID else { return }
        terminalRegistrations.removeValue(forKey: paneID)
    }

    func splitCurrentPane(direction: PaneSplitDirection, cwd: String) {
        guard let paneID = snapshot?.terminal.paneID else {
            bridgeError = "pane.no_current_pane: Select a terminal pane before splitting"
            return
        }
        guard !cwd.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            bridgeError = "pane.no_checkout_path: The selected checkout path is empty"
            return
        }
        dispatch(kind: "create_pane", payload: [
            "tab_id": paneID,
            "cwd": cwd,
            "command": NSNull(),
            "direction": direction.rawValue,
        ])
    }

    func toggleCurrentPaneZoom() {
        guard let paneID = snapshot?.terminal.paneID else {
            bridgeError = "pane.no_current_pane: Select a terminal pane before toggling zoom"
            return
        }
        togglePaneZoom(paneID)
    }

    func togglePaneZoom(_ paneID: String) {
        dispatch(kind: "toggle_zoom", payload: ["pane_id": paneID])
    }

    func toggleConversation(_ paneID: String) {
        dispatch(kind: "toggle_conversation", payload: ["pane_id": paneID])
    }

    func resizePane(_ paneID: String, direction: PaneResizeDirection, amount: Double) {
        guard amount.isFinite, amount >= 0.001 else { return }
        dispatch(kind: "resize_pane", payload: [
            "pane_id": paneID,
            "direction": direction.rawValue,
            "amount": min(amount, 0.5),
        ])
    }

    func closePane(_ paneID: String, confirmed: Bool) {
        dispatch(kind: "close_pane", payload: [
            "pane_id": paneID,
            "confirmed": confirmed,
        ])
    }

    func forkPane(_ paneID: String) {
        dispatch(kind: "fork_pane", payload: ["pane_id": paneID])
    }

    @discardableResult
    func focusRemotePane(
        targetID: String,
        paneID: String,
        requestID: String? = nil
    ) -> CoreDispatchOutcome {
        dispatchRemoteControl(
            targetID: targetID,
            action: "focus_pane",
            extra: ["pane_id": paneID],
            requestID: requestID
        )
    }

    func splitRemotePane(targetID: String, paneID: String, direction: PaneSplitDirection) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "split_pane",
            extra: [
                "pane_id": paneID,
                "direction": direction.rawValue,
            ]
        )
    }

    func toggleRemotePaneZoom(targetID: String, paneID: String) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "toggle_pane_zoom",
            extra: ["pane_id": paneID]
        )
    }

    func closeRemotePane(targetID: String, paneID: String, confirmed: Bool) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "close_pane",
            extra: [
                "pane_id": paneID,
                "confirmed": confirmed,
            ]
        )
    }

    func focusRemoteWorkspace(targetID: String, workspaceID: String) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "focus_workspace",
            extra: ["workspace_id": workspaceID]
        )
    }

    func focusRemoteTab(targetID: String, tabID: String) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "focus_tab",
            extra: ["tab_id": tabID]
        )
    }

    func createRemoteTab(
        targetID: String,
        workspaceID: String,
        cwd: String,
        label: String
    ) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "create_tab",
            extra: [
                "workspace_id": workspaceID,
                "cwd": cwd,
                "label": label,
            ]
        )
    }

    func closeRemoteTab(targetID: String, tabID: String, confirmed: Bool) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "close_tab",
            extra: [
                "tab_id": tabID,
                "confirmed": confirmed,
            ]
        )
    }

    @discardableResult
    private func dispatchRemoteControl(
        targetID: String,
        action: String,
        extra: [String: Any] = [:],
        requestID: String? = nil
    ) -> CoreDispatchOutcome {
        let reportsPaneFocusOutcome = requestID != nil
        var payload: [String: Any] = [
            "target_id": targetID,
            "request_id": requestID ?? UUID().uuidString,
            "action": action,
        ]
        if reportsPaneFocusOutcome {
            payload["report_pane_focus_outcome"] = true
        }
        for (key, value) in extra {
            payload[key] = value
        }
        return dispatch(kind: "remote_control", payload: payload)
    }

    func recordBrowserStatus(_ receipt: BrowserRuntimeReceipt) {
        let checkedMilliseconds = UInt64(
            (ISO8601DateFormatter().date(from: receipt.checkedAt)?.timeIntervalSince1970 ?? Date().timeIntervalSince1970) * 1_000
        )
        dispatch(kind: "browser_status", payload: [
            "state": receipt.phase.rawValue,
            "profile": receipt.profile,
            "current_url": receipt.currentURL.map { $0 as Any } ?? NSNull(),
            "current_title": receipt.currentTitle.map { $0 as Any } ?? NSNull(),
            "message": receipt.message,
            "last_checked_at_unix_ms": NSNumber(value: checkedMilliseconds),
        ])
    }

    /// One clicked path, with everything it changes on screen decided by the
    /// core in a single event.
    ///
    /// This is deliberately not a sequence of `focusCheckout`, `persistUIState`
    /// and `openFile`: dispatch is fire-and-forget, so those would arrive as
    /// separate frames and a refusal partway would leave the screen half
    /// moved. The shell's part is the filesystem question - what the path is,
    /// and whose checkout it belongs to - which cannot run under the runtime
    /// mutex.
    func revealPath(_ url: URL, workspaceID: String, checkoutID: String, isDirectory: Bool) {
        dispatch(kind: "reveal_path", payload: [
            "path": url.path,
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
            "is_directory": isDirectory,
        ])
    }

    func openFile(_ url: URL, workspaceID: String, checkoutID: String) {
        dispatch(kind: "file_open", payload: [
            "path": url.path,
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
        ])
    }

    func focusFileTab(_ tabID: String) {
        dispatch(kind: "file_focus", payload: ["tab_id": tabID])
    }

    /// The explorer's five filesystem changes. Each is one event: the core
    /// decides the paths, runs the call off the runtime mutex, and reports
    /// through `explorerOperation`, so the tree never touches the disk and
    /// never shows a half-applied change.
    func createFile(root: URL, parent: URL, name: String) {
        dispatch(kind: "file_create", payload: ["root": root.path, "parent": parent.path, "name": name])
    }

    func createDirectory(root: URL, parent: URL, name: String) {
        dispatch(kind: "dir_create", payload: ["root": root.path, "parent": parent.path, "name": name])
    }

    func renamePath(root: URL, path: URL, name: String) {
        dispatch(kind: "path_rename", payload: ["root": root.path, "path": path.path, "name": name])
    }

    func movePath(root: URL, path: URL, destination: URL) {
        dispatch(kind: "path_move", payload: ["root": root.path, "path": path.path, "destination": destination.path])
    }

    /// Only the confirmed modal calls this; the tree itself never does.
    /// `selectAfter` is the row the tree chose to select once the item is
    /// gone, so the selection lands in the same frame as the removal.
    func trashPath(root: URL, path: URL, selectAfter: URL, inode: UInt64?) {
        var payload: [String: Any] = ["root": root.path, "path": path.path, "select_after": selectAfter.path]
        if let inode {
            payload["inode"] = inode
        }
        dispatch(kind: "path_trash", payload: payload)
    }

    func closeFileTab(_ tabID: String) {
        var payload: [String: Any] = ["tab_id": tabID]
        if let pendingSave = takePendingFileSave(for: tabID) {
            payload["pending_save"] = pendingSave
        }
        dispatch(kind: "file_close", payload: payload)
    }

    func setFileView(tabID: String, preview: Bool, wrap: Bool) {
        dispatch(kind: "file_view", payload: ["tab_id": tabID, "markdown_preview": preview, "wrap": wrap])
    }

    func updateDraft(_ contents: String) {
        dispatch(kind: "file_draft", payload: ["contents_utf8": contents])
    }

    func saveFile(_ contents: String) {
        guard let editor = snapshot?.editor,
              let tabID = editor.activeTabID,
              let path = editor.path
        else { return }
        dispatch(kind: "file_save", payload: [
            "tab_id": tabID,
            "path": path,
            "contents_utf8": contents,
            "expected_modified_at_unix_ms": editor.openedModifiedAt.map { NSNumber(value: $0) as Any } ?? NSNull(),
        ])
    }

    func scheduleFileSave(_ contents: String) {
        guard let editor = snapshot?.editor, let tabID = editor.activeTabID, let path = editor.path else { return }
        if let previous = pendingFileSavePayload?["tab_id"] as? String, previous != tabID {
            flushPendingFileSave()
        }
        pendingFileSave?.cancel()
        pendingFileSavePayload = [
            "tab_id": tabID, "path": path, "contents_utf8": contents,
            "expected_modified_at_unix_ms": editor.openedModifiedAt.map { NSNumber(value: $0) as Any } ?? NSNull(),
        ]
        pendingFileSave = Task { @MainActor [weak self] in
            do { try await Task.sleep(for: .milliseconds(450)) } catch { return }
            guard !Task.isCancelled else { return }
            self?.flushPendingFileSave()
        }
    }

    func flushPendingFileSave(for tabID: String? = nil) {
        guard let payload = takePendingFileSave(for: tabID) else { return }
        dispatch(kind: "file_save", payload: payload)
    }

    private func takePendingFileSave(for tabID: String? = nil) -> [String: Any]? {
        guard let payload = pendingFileSavePayload else { return nil }
        if let tabID, payload["tab_id"] as? String != tabID { return nil }
        pendingFileSave?.cancel()
        pendingFileSave = nil
        pendingFileSavePayload = nil
        return payload
    }

    func resolveConflict(_ action: String) {
        dispatch(kind: "file_conflict", payload: ["action": action])
    }

    func persistUIState(
        leftSidebarVisible: Bool? = nil,
        rightPanelVisible: Bool? = nil,
        rightPanelSection: RightPanelSection? = nil,
        expandedPaths: [String]? = nil,
        collapsedWorkspaceIDs: [String]? = nil,
        collapsedCheckoutIDs: [String]? = nil,
        selectedPath: String? = nil,
        selectedPaneID: String? = nil,
        focusedCheckoutID: String? = nil,
        shortcutBindings: [String: String]? = nil,
        accentHex: String? = nil,
        fontSize: Double? = nil,
        usageWindowVisible: Bool? = nil,
        usagePopoverOpen: Bool? = nil
    ) {
        let current = snapshot?.uiState
        let effectivePath = selectedPath ?? current?.selectedPath
        let effectivePaneID = selectedPaneID ?? current?.selectedPaneID
        var payload: [String: Any] = [
            "left_sidebar_visible": leftSidebarVisible ?? current?.leftSidebarVisible ?? true,
            "right_panel_visible": rightPanelVisible ?? current?.rightPanelVisible ?? true,
            "right_panel_section": (rightPanelSection ?? current?.rightPanelSection ?? .explorer)
                .rawValue,
            "expanded_paths": expandedPaths ?? current?.expandedPaths ?? [],
            "collapsed_workspace_ids": collapsedWorkspaceIDs ?? current?.collapsedWorkspaceIDs ?? [],
            "selected_path": effectivePath.map { $0 as Any } ?? NSNull(),
            "selected_pane_id": effectivePaneID.map { $0 as Any } ?? NSNull(),
            "shortcut_bindings": shortcutBindings ?? current?.shortcutBindings ?? [:],
            "accent_hex": accentHex ?? current?.accentHex ?? "#B9FF66",
            "font_size": fontSize ?? current?.fontSize ?? 13,
        ]
        // Registration events own durable workspace/device lists. A generic
        // UI-state save must not replay a stale snapshot and erase the
        // session-derived temporary catalog.
        if let collapsedCheckoutIDs {
            payload["collapsed_checkout_ids"] = collapsedCheckoutIDs
        }
        if let focusedCheckoutID {
            // This is the one explicit local-selection anchor used after a
            // terminal launcher returns. It never asks Herdr to change focus.
            payload["focused_checkout_id"] = focusedCheckoutID
        }
        if let usageWindowVisible {
            payload["usage_window_visible"] = usageWindowVisible
        }
        if let usagePopoverOpen {
            payload["usage_popover_open"] = usagePopoverOpen
        }
        if selectedPaneID != nil || focusedCheckoutID != nil {
            HideLaunchTrace.mark(
                "core.dispatch.anchor",
                detail: "selected_pane_id=\(effectivePaneID ?? "nil") focused_checkout_id=\(focusedCheckoutID ?? "preserve")"
            )
        }
        dispatch(kind: "ui_state_update", payload: payload)
    }

    func setUsageWindowVisible(_ visible: Bool) {
        persistUIState(usageWindowVisible: visible)
    }

    func setUsagePopoverOpen(_ open: Bool) {
        persistUIState(usagePopoverOpen: open)
    }

    /// Selects the changed file whose diff the changes view shows, or clears
    /// the selection when `path` is nil.
    func selectChangedFile(path: String?, committed: Bool = false) {
        dispatch(
            kind: "changes_select",
            payload: [
                "path": path.map { $0 as Any } ?? NSNull(),
                "committed": committed,
            ]
        )
    }

    /// The card's refresh button: the pull-request lookup, the worktree
    /// counts, and the size are all read again.
    func refreshCheckoutCard() {
        dispatch(kind: "card_refresh", payload: [:])
    }

    /// Measures the selected checkout again, for the confirmation that states
    /// the size it is about to delete.
    func measureCheckoutDisk() {
        dispatch(kind: "card_measure_disk", payload: [:])
    }

    @discardableResult
    func dispatch(kind: String, payload: [String: Any]) -> CoreDispatchOutcome {
        if CoreDispatchRoutingPolicy.blocks(
            kind: kind,
            payload: payload,
            remoteDeviceID: commandDevice.isRemote ? commandDevice.id : nil
        ) {
            let message = "device.route_blocked: \(commandDevice.label) is selected. \(kind) was not sent to the local Herdr session."
            routingError = message
            bridgeError = message
            HideLaunchTrace.mark(
                "core.dispatch.blocked",
                detail: "kind=\(kind) device_id=\(commandDevice.id)"
            )
            return .rejected(message)
        }
        if LocalHerdrMutationDispatchPolicy.requiresConnectedHerdr(
            kind: kind,
            whenDeviceIsRemote: commandDevice.isRemote
        ) {
            let readiness = localHerdrMutationReadiness
            guard case .connected = readiness else {
                let message = readiness.message
                bridgeError = message
                if LocalHerdrMutationDispatchPolicy.presentsRejection(kind: kind) {
                    localHerdrMutationRejectionHandler?(readiness)
                }
                HideLaunchTrace.mark(
                    "core.dispatch.blocked",
                    detail: "kind=\(kind) reason=local_herdr_not_ready"
                )
                return .rejected(message)
            }
        }
        guard let core else {
            let message = "Hide is still starting. Try again when the Herdr status is available."
            bridgeError = message
            return .rejected(message)
        }
        if let detail = dispatchTraceDetail(kind: kind, payload: payload) {
            HideLaunchTrace.mark("core.dispatch", detail: detail)
        }
        let envelope: [String: Any] = [
            "schema_version": coreSchemaVersion,
            "kind": kind,
            "payload": payload,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: envelope) else {
            let message = "Could not encode \(kind) event"
            bridgeError = message
            return .rejected(message)
        }
        data.withUnsafeBytes { buffer in
            herdr_core_dispatch(core, buffer.bindMemory(to: UInt8.self).baseAddress, data.count)
        }
        return .accepted
    }

    private func dispatchTraceDetail(kind: String, payload: [String: Any]) -> String? {
        switch kind {
        case "focus_checkout":
            return "kind=focus_checkout workspace_id=\(traceValue(payload, key: "workspace_id")) checkout_id=\(traceValue(payload, key: "checkout_id"))"
        case "ui_state_update":
            return "kind=ui_state_update selected_pane_id=\(traceValue(payload, key: "selected_pane_id")) focused_checkout_id=\(traceValue(payload, key: "focused_checkout_id")) workspace_registrations=\(payload["workspace_registrations"] == nil ? "omitted" : "present")"
        case "focus_tab", "focus_pane":
            // The two view-state dispatches. Hide decides both of them itself,
            // so the interval from this mark to the projection that follows is
            // the whole of what the operator waits for, with no Herdr round
            // trip in it. Measuring it any other way costs the synthetic input
            // harness, which on this machine is over a hundred milliseconds
            // before a keystroke even reaches the app.
            return "kind=\(kind) tab_id=\(traceValue(payload, key: "tab_id")) pane_id=\(traceValue(payload, key: "pane_id")) origin=\(traceValue(payload, key: "origin"))"
        case "terminal_resize":
            // When a view first reports its size is when a pane whose size is
            // not yet known can attach, so the launch trace has to be able to
            // see it. A view reports only when its cell count changes, so this
            // is not a per-frame line.
            return "kind=terminal_resize pane_id=\(traceValue(payload, key: "pane_id")) rows=\(traceValue(payload, key: "rows")) cols=\(traceValue(payload, key: "cols"))"
        default:
            return nil
        }
    }

    private func traceValue(_ payload: [String: Any], key: String) -> String {
        guard let value = payload[key], !(value is NSNull) else { return "nil" }
        return String(describing: value)
    }

    func environmentState(for key: String) -> String? {
        snapshot?.status.environment.first(where: { $0.key == key })?.state
    }

    private func refreshSnapshot() {
        guard let core else { return }
        var requestedRevision = haveRevision
        #if DEBUG
        // Recording requests the existing full wire format. Replaying is only
        // available through the isolated UI fixture, never the live client.
        if verificationSnapshotOutput != nil { requestedRevision = 0 }
        #endif
        let owned = herdr_core_snapshot(core, requestedRevision, lastTerminalSequence)
        defer { herdr_core_free_bytes(owned) }
        guard let pointer = owned.ptr, owned.len > 0 else {
            bridgeError = "herdr_core_snapshot returned empty bytes"
            return
        }
        do {
            var data = Data(bytes: pointer, count: owned.len)
            #if DEBUG
            if let verificationSnapshotPath {
                data = try Data(contentsOf: URL(fileURLWithPath: verificationSnapshotPath))
            }
            if let verificationSnapshotOutput {
                try data.write(to: URL(fileURLWithPath: verificationSnapshotOutput), options: .atomic)
            }
            #endif
            let decoded = try JSONDecoder().decode(CoreSnapshotDelta.self, from: data)
            if decoded.revision != lastLoggedSnapshotRevision {
                lastLoggedSnapshotRevision = decoded.revision
                HideLaunchTrace.mark(
                    "core.snapshot.refresh",
                    detail: "revision=\(decoded.revision) rest=\(decoded.rest != nil) editor=\(decoded.editor != nil) terminal_sequence=\(decoded.terminalSequence)"
                )
            }
            if decoded.chunksDropped {
                HideLaunchTrace.mark(
                    "terminal.chunks_dropped",
                    detail: "cursor \(lastTerminalSequence) behind ring"
                )
            }
            if let rest = decoded.rest {
                // The protocol stamps every section ahead of a fresh cursor,
                // so a missing editor here is a contract violation, not a
                // state to default over.
                guard let editor = decoded.editor ?? snapshot?.editor else {
                    bridgeError = "delta.protocol: first response carried no editor"
                    return
                }
                apply(composed: CoreSnapshot(
                    schemaVersion: decoded.schemaVersion,
                    navigator: rest.navigator,
                    zoomed: rest.zoomed,
                    paneLayouts: rest.paneLayouts,
                    terminal: rest.terminal,
                    editor: editor,
                    changes: decoded.changes ?? snapshot?.changes ?? .empty,
                    card: rest.card,
                    find: decoded.find,
                    uiState: rest.uiState,
                    status: rest.status,
                    pet: rest.pet,
                    recentClosed: rest.recentClosed,
                    gitWorktrees: rest.gitWorktrees, gitWorktreesLoading: rest.gitWorktreesLoading,
                    gitWorktreesRemote: rest.gitWorktreesRemote, worktreeRemoval: rest.worktreeRemoval,
                    taskOperation: rest.taskOperation,
                    explorerOperation: rest.explorerOperation
                ))
            } else if decoded.editor != nil
                || decoded.changes != nil
                || decoded.find != snapshot?.find
            {
                guard let current = snapshot else {
                    bridgeError = "delta.protocol: a section arrived before the first full snapshot"
                    return
                }
                // Find state arrives on every response, so it is compared
                // rather than applied: republishing on each one would rebuild
                // the view for every terminal chunk, which is the invariant
                // chunk-only deltas exist to protect.
                snapshot = current.replacing(
                    editor: decoded.editor,
                    changes: decoded.changes,
                    find: decoded.find
                )
            }
            haveRevision = decoded.revision
            for chunk in decoded.chunks
                .filter({ $0.sequence > lastTerminalSequence })
                .sorted(by: { $0.sequence < $1.sequence })
            {
                lastTerminalSequence = chunk.sequence
                guard let data = Data(base64Encoded: chunk.bytesBase64) else {
                    bridgeError = "terminal.invalid_base64: sequence \(chunk.sequence)"
                    continue
                }
                let dropped = pendingTerminalBytes.append(TerminalDelivery(bytes: [UInt8](data), frame: chunk.frame, inputSent: chunk.inputSent), for: chunk.paneID)
                if dropped > 0 {
                    HideLaunchTrace.mark(
                        "terminal.buffer.dropped",
                        detail: "pane_\(chunk.paneID)_chunks_\(dropped)"
                    )
                }
                drainPendingTerminalBytes(for: chunk.paneID)
            }
        } catch {
            bridgeError = "Snapshot decode failed: \(error.localizedDescription)"
        }
    }

    /// Replaces the composed snapshot and runs the side effects that watch
    /// it. Only rest-section changes reach here; chunk-only deltas never
    /// touch the published snapshot.
    private func apply(composed decoded: CoreSnapshot) {
        var decoded = decoded
        decoded.navigationRevision = (snapshot?.navigationRevision ?? 0) &+ 1
        if decoded.status.herdr.state != lastLoggedHerdrState {
            lastLoggedHerdrState = decoded.status.herdr.state
            HideLaunchTrace.mark("herdr.status", detail: decoded.status.herdr.state)
        }
        if let lastError = decoded.status.lastError {
            if lastError.kind != lastLoggedErrorKind {
                lastLoggedErrorKind = lastError.kind
                HideLaunchTrace.mark("core.error", detail: lastError.kind)
            }
        } else {
            lastLoggedErrorKind = nil
        }
        let focusedCheckout = decoded.navigator.focusedCheckoutID.flatMap { checkoutID in
            decoded.navigator.workspaces
                .lazy
                .flatMap(\.checkouts)
                .first(where: { $0.id == checkoutID })
        }
        // The layout the canvas draws is the visible tab's, so that is what
        // the projection line names. A layout no longer needs to be tested
        // against the focused checkout: it is found through that checkout's
        // own active tab or not at all.
        let visibleLayout = focusedCheckout?.activeTabID.flatMap { tabID in
            decoded.paneLayouts.first(where: { $0.tabID == tabID })
        }
        let projection = [
            "focused_workspace_id=\(decoded.navigator.focusedWorkspaceID ?? "nil")",
            "focused_checkout_id=\(decoded.navigator.focusedCheckoutID ?? "nil")",
            "checkout_workspace_id=\(focusedCheckout?.workspaceID ?? "nil")",
            "layout_workspace_id=\(visibleLayout?.workspaceID ?? "nil")",
            "layout_tab_id=\(visibleLayout?.tabID ?? "nil")",
            "layout_pane_ids=\(visibleLayout?.root.paneIDs.joined(separator: ",") ?? "nil")",
            "terminal_pane_id=\(decoded.terminal.paneID ?? "nil")",
            "layout_count=\(decoded.paneLayouts.count)"
        ].joined(separator: " ")
        if projection != lastLoggedProjection {
            lastLoggedProjection = projection
            HideLaunchTrace.mark("core.snapshot.projection", detail: projection)
        }
        // A pane whose session the core released, or that has left the
        // session, redraws from Herdr's own full frame when it is next shown.
        // Its held bytes are frames nobody will draw, so they go now rather
        // than being carried for the life of the process.
        //
        // Which panes still exist is read from Herdr's layouts, not from the
        // transport projection. The projection is emptied while a selection is
        // in progress - `clear_terminal_projection` in the core does exactly
        // that, and deliberately leaves the layouts alone - so a tick taken in
        // that moment lists no pane at all. Dropping every held frame on that
        // is what left a pane blank after a zoom or a switch to a tab of
        // several panes: the full frame was thrown away while its view was
        // still being built, and the next small delta drew on an empty grid.
        // An empty layout set is no answer either, so nothing is dropped then.
        if let keep = PendingTerminalRetention.keep(
            layouts: decoded.paneLayouts,
            transportPanes: decoded.terminal.panes
        ) {
            for paneID in pendingTerminalBytes.retain(paneIDs: keep) {
                HideLaunchTrace.mark("terminal.buffer.cleared", detail: "pane_\(paneID)")
            }
        }
        let previousFocusedPaneID = snapshot?.focusedPaneID
        snapshot = decoded
        bridgeError = routingError
            ?? decoded.status.lastError.map { "\($0.kind): \($0.message)" }
            ?? startupDiagnostic
        restorePaneSelectionIfNeeded(decoded)
        let authoritativeFocusedPaneID = decoded.focusedPaneID
        if authoritativeFocusedPaneID != previousFocusedPaneID,
           let authoritativeFocusedPaneID {
            DispatchQueue.main.async { [weak self] in
                self?.terminalRegistrations[authoritativeFocusedPaneID]?.focus()
            }
        }
    }

    /// Restores the persisted pane selection once on launch so the terminal
    /// reattaches to the pane the user last worked in. A pane that no longer
    /// exists fails through the normal attach error path.
    private func restorePaneSelectionIfNeeded(_ decoded: CoreSnapshot) {
        guard !restoredPaneSelection else { return }
        restoredPaneSelection = true
        guard decoded.terminal.paneID == nil,
              let persisted = decoded.uiState.selectedPaneID else { return }
        focusPane(persisted, origin: .restore)
    }

    private func drainPendingTerminalBytes(for paneID: String) {
        guard let registration = terminalRegistrations[paneID],
              let pending = pendingTerminalBytes.take(paneID)
        else { return }
        for delivery in pending {
            let bytes = delivery.bytes
            registration.receive(delivery)
            // The launch is over when a terminal first has a frame to draw.
            // The core writes `ESC c` immediately before a matching full
            // frame to reset the parser, so that
            // chunk marks the request, not the frame. Bytes held for a pane
            // with no view yet are not the moment either, which is why this
            // is here and not where the snapshot is decoded.
            if bytes != Self.terminalGridReset {
                HideLaunchTrace.markOnce("first_terminal_frame", detail: "pane_\(paneID)")
            }
        }
    }

    /// The grid reset the core writes when it requests a terminal session.
    private static let terminalGridReset: [UInt8] = [0x1b, 0x63]

    #if DEBUG
    private func seedVerificationFixture() {
        let paneID = "fixture-working"
        let workspaceID = "fixture-workspace"
        let tabID = "fixture-tab"
        let agents: [[String: Any]] = [
            Self.fixtureAgent("error", "×", "1755000007000", "Build failed", "12s", "Core", "codex", "status_error_new"),
            Self.fixtureAgent("question", "?", "1755000006000", "Choose persistence scope", "4m", "UI", "claude", "status_question_new"),
            Self.fixtureAgent("approval", "!", "1755000005000", "Approve local save", "8m", "Explorer", "codex", "status_approval_new"),
            Self.fixtureAgent("done", "●", "1755000004000", "Sidebar contract complete", "2h", "Agents", "claude", "status_done_new"),
            Self.fixtureAgent("working", "●", "1755000003000", "Connecting Rust bytes", "18s", "Terminal", "codex", "status_working"),
            Self.fixtureAgent("idle", "○", "1755000002000", "Reviewed fixture", "3d", "Verify", "claude", "status_idle"),
            Self.fixtureAgent("unknown", "~", "1755000001000", "Awaiting lifecycle token", "9m", "Other", "unknown", "status_unknown"),
        ]
        dispatch(kind: "create_workspace", payload: [
            "path": workspaceRoot.path,
            "label": "hide rebrand",
            "initialize_git": false,
        ])
        dispatch(kind: "session_snapshot", payload: [
            "focused_pane_id": paneID,
            "agents": agents,
            "workspaces": [[
                "workspace_id": workspaceID,
                "label": "hide rebrand",
                "active_tab_id": tabID,
            ]],
            "tabs": [["tab_id": tabID, "workspace_id": workspaceID, "label": "Round 3"]],
            "panes": [["pane_id": paneID, "cwd": workspaceRoot.path]],
            "layouts": [[
                "workspace_id": workspaceID,
                "tab_id": tabID,
                "zoomed": false,
                "area": ["x": 0, "y": 0, "width": 120, "height": 60],
                "focused_pane_id": paneID,
                "panes": [[
                    "pane_id": paneID,
                    "rect": ["x": 0, "y": 0, "width": 120, "height": 60],
                ]],
                "splits": [],
            ]],
        ])
        let banner = "\u{001B}[1;36mherdr-core ↔ SwiftTerm\u{001B}[0m\r\nLocal byte bridge ready. IME V9 remains blocked.\r\n\r\n"
        dispatch(kind: "terminal_output", payload: [
            "pane_id": paneID,
            "bytes_base64": Data(banner.utf8).base64EncodedString(),
        ])
    }

    private static func fixtureAgent(
        _ id: String,
        _ symbol: String,
        _ activity: String,
        _ progress: String,
        _ elapsed: String,
        _ workspace: String,
        _ agent: String,
        _ statusToken: String
    ) -> [String: Any] {
        [
            "id": id,
            "pane_id": "fixture-\(id)",
            "workspace_label": workspace,
            "agent": agent,
            "agent_status": id == "working" ? "working" : id == "idle" ? "idle" : "unknown",
            "tokens": [
                statusToken: symbol,
                "activity": activity,
                "progress": progress,
                "elapsed": elapsed,
            ],
        ]
    }
    #endif

    /// The release bundle identifier. A build carrying any other identifier is
    /// a per-worktree instance (see `macos/scripts/build_dev_app.sh`).
    nonisolated static let releaseBundleIdentifier = "me.grab.hide"

    /// Where this instance keeps its persisted UI state.
    ///
    /// Two builds may run at once - one per worktree - and they must not
    /// overwrite each other's selected pane, expanded folders, panel
    /// visibility, and pet position. The bundle identifier is what already
    /// separates those instances to the system, so it separates their state
    /// too. The release identifier keeps the original path so an upgrade does
    /// not lose the state the user already has.
    nonisolated static func defaultStatePath(
        bundleIdentifier: String? = Bundle.main.bundleIdentifier
    ) -> String {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        let root = base.appendingPathComponent("hide")
        guard let identifier = bundleIdentifier, identifier != releaseBundleIdentifier else {
            return root.appendingPathComponent("state.json").path
        }
        return root
            .appendingPathComponent("instances")
            .appendingPathComponent(identifier)
            .appendingPathComponent("state.json")
            .path
    }

}
