import AppKit
import Combine
import Foundation

enum ShellSurface: String, CaseIterable, Hashable {
    case agents
    case terminal
    case workbench

    var title: String {
        switch self {
        case .agents:
            "Agents"
        case .terminal:
            "Terminal"
        case .workbench:
            "Workbench"
        }
    }
}

enum PaneSplitDirection: String, Decodable, Equatable, Sendable {
    case right
    case down
}

enum CheckoutSelectionAction: Equatable {
    case focusExisting
    case startTerminal
}

enum CheckoutSelectionPolicy {
    static func action(for checkout: CoreCheckoutSnapshot) -> CheckoutSelectionAction {
        checkout.tabs.flatMap { $0.panes }.isEmpty ? .startTerminal : .focusExisting
    }
}

enum CheckoutStartState {
    case idle
    case starting
    case started
    case failed(String)
}

enum TerminalLayoutPolicy {
    static func belongs(
        layout: CorePaneLayoutSnapshot,
        to checkout: CoreCheckoutSnapshot
    ) -> Bool {
        belongs(
            workspaceID: layout.workspaceID,
            tabID: layout.tabID,
            paneIDs: layout.root.paneIDs,
            checkout: checkout
        )
    }

    static func belongs(
        workspaceID: String,
        tabID: String,
        paneIDs: [String],
        checkout: CoreCheckoutSnapshot
    ) -> Bool {
        // `layout.workspace_id` is Herdr's live session workspace ID, while
        // `checkout.workspaceID` is Hide's catalog workspace ID. They are
        // intentionally different identity domains. The Herdr tab ID and
        // its complete pane set are the stable cross-domain projection key.
        _ = workspaceID
        guard let tab = checkout.tabs.first(where: { $0.id == tabID }) else { return false }
        return Set(paneIDs) == Set(tab.panes.map(\.id))
    }
}

@MainActor
final class ShellModel: ObservableObject {
    @Published var activeSurface: ShellSurface = .terminal
    @Published var consequenceNotice: ConsequenceNotice?
    @Published var consequenceResult: String?
    @Published var showNewWorkspace = false
    @Published var showNewAgent = false
    @Published var showSearch = false
    @Published var showSettings = false
    @Published var workspaceToRemove: CoreWorkspaceSnapshot?
    @Published var interactionNotice: String?
    @Published var selectedAgentKind = "claude"
    @Published var selectedAgentCheckoutID: String?
    @Published var selectedAgentDeviceID = "local"
    @Published var agentBypassWarnings = false
    @Published private(set) var paneShortcuts: [PaneCommand: PaneShortcut]
    @Published private(set) var shortcutErrors: [PaneCommand: String] = [:]
    @Published private(set) var shortcutDiagnostic: String?
    @Published private(set) var checkoutStartState: CheckoutStartState = .idle
    let core: CoreBridge
    let browser: BrowserRuntimeModel
    let remote: RemoteRuntimeModel
    private var coreSubscription: AnyCancellable?
    private var browserSubscription: AnyCancellable?
    private var remoteSubscription: AnyCancellable?
    private var pendingPaneCloseTarget: PaneCloseTarget?
    private var lastRemoteDevice: CoreDeviceSnapshot?
    @Published private(set) var activeRemoteDevice: CoreDeviceSnapshot?
    private var pendingCheckoutStarts: Set<String> = []

    init(
        core: CoreBridge = CoreBridge(),
        browser: BrowserRuntimeModel = BrowserRuntimeModel(),
        remote: RemoteRuntimeModel = RemoteRuntimeModel()
    ) {
        self.core = core
        self.browser = browser
        self.remote = remote
        agentBypassWarnings = core.snapshot?.uiState.bypassWarnings ?? false
        let shortcutResolution = PaneShortcutPolicy.resolve(
            stored: core.snapshot?.uiState.shortcutBindings ?? [:]
        )
        paneShortcuts = shortcutResolution.bindings
        shortcutDiagnostic = shortcutResolution.diagnostic ?? Self.uiStateDiagnostic(core.snapshot)
        coreSubscription = core.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
        browserSubscription = browser.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
        remoteSubscription = remote.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
        browser.environmentStateProvider = { [weak core] key in
            core?.environmentState(for: key)
        }
        remote.environmentStateProvider = { [weak core] key in
            core?.environmentState(for: key)
        }
        remote.onActionFailure = { [weak self] message in
            self?.interactionNotice = message
        }
        browser.onReceipt = { [weak core] receipt in
            core?.recordBrowserStatus(receipt)
        }
    }

    var workspaces: [CoreWorkspaceSnapshot] {
        if isRemoteContext {
            return remote.navigation?.workspaces ?? []
        }
        return core.snapshot?.navigator.workspaces ?? []
    }

    var devices: [CoreDeviceSnapshot] {
        core.snapshot?.navigator.devices ?? []
    }

    var agents: [SidebarAgent] {
        if isRemoteContext {
            return remote.navigation?.agents ?? []
        }
        return core.snapshot?.navigator.agents ?? []
    }

    var agentsNeedingAttention: [SidebarAgent] {
        SidebarGrouping.needingAttention(agents)
    }

    func agents(in checkout: CoreCheckoutSnapshot) -> [SidebarAgent] {
        SidebarGrouping.agents(agents, in: checkout)
    }

    var focusedWorkspace: CoreWorkspaceSnapshot? {
        if isRemoteContext {
            guard let navigation = remote.navigation else { return nil }
            if let id = navigation.focusedWorkspaceID {
                return navigation.workspaces.first(where: { $0.id == id }) ?? navigation.workspaces.first
            }
            return navigation.workspaces.first
        }
        guard let id = core.snapshot?.navigator.focusedWorkspaceID else { return nil }
        return workspaces.first(where: { $0.id == id })
    }

    var focusedCheckout: CoreCheckoutSnapshot? {
        if isRemoteContext {
            guard let navigation = remote.navigation else { return nil }
            if let id = navigation.focusedCheckoutID,
               let checkout = navigation.workspaces.lazy.compactMap({ workspace in
                   workspace.checkouts.first(where: { $0.id == id })
               }).first {
                return checkout
            }
            return focusedWorkspace?.checkouts.first
        }
        if let id = core.snapshot?.navigator.focusedCheckoutID,
           let checkout = workspaces.lazy.compactMap({ workspace in
               workspace.checkouts.first(where: { $0.id == id })
           }).first {
            return checkout
        }
        return nil
    }

    var focusedTabs: [CoreTabSnapshot] {
        focusedCheckout?.tabs ?? []
    }

    var focusedTab: CoreTabSnapshot? {
        guard let checkout = focusedCheckout else { return nil }
        let preferredTabID = isRemoteContext
            ? remote.navigation?.focusedTabID
            : core.snapshot?.paneLayout?.tabID
        if let preferredTabID,
           let tab = checkout.tabs.first(where: { $0.id == preferredTabID }) {
            return tab
        }
        return checkout.tabs.first
    }

    var focusedPanes: [CorePaneSnapshot] {
        focusedTab?.panes ?? []
    }

    var focusedPaneGridItems: [PaneGridItem] {
        if isRemoteContext {
            guard paneProjectionNotice == nil,
                  let layout = remote.navigation?.focusedPaneLayout
            else { return [] }
            let items = PaneGridPresentation.items(
                remoteLayout: layout,
                focusedPaneID: focusedPaneID
            )
            return items
        } else if let layout = focusedPaneLayout {
            return PaneGridPresentation.items(
                layout: layout,
                focusedPaneID: focusedPaneID
            )
        }
        return PaneGridPresentation.uniformItems(
            paneIDs: focusedPanes.map(\.id),
            focusedPaneID: focusedPaneID
        )
    }

    var focusedPaneLayout: CorePaneLayoutSnapshot? {
        guard let layout = core.snapshot?.paneLayout,
              let checkout = focusedCheckout,
              TerminalLayoutPolicy.belongs(layout: layout, to: checkout)
        else { return nil }
        return layout
    }

    var focusedPath: URL? {
        if isRemoteContext {
            guard let path = focusedCheckout?.path, !path.isEmpty else { return nil }
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        guard let path = core.snapshot?.navigator.rootPath else { return nil }
        return URL(fileURLWithPath: path, isDirectory: true)
    }

    var localProjectionNotice: String? {
        guard !isRemoteContext,
              let error = core.snapshot?.status.lastError,
              error.kind == "pane.projection_unavailable"
        else { return nil }
        return "\(error.kind): \(error.message)"
    }

    var paneProjectionNotice: String? {
        guard isRemoteContext, !focusedPanes.isEmpty else {
            return localProjectionNotice
        }
        guard let layout = remote.navigation?.focusedPaneLayout else {
            return "remote.pane_layout_unavailable: The selected remote tab has panes but no layout in the Herdr snapshot. Retry the Mac mini connection."
        }
        let expectedPaneIDs = Set(focusedPanes.map(\.id))
        let layoutPaneIDs = Set(layout.frames.map(\.paneID))
        guard !layout.frames.isEmpty, layoutPaneIDs == expectedPaneIDs else {
            return "remote.pane_layout_mismatch: The selected remote tab's panes do not match its layout. Retry the Mac mini connection."
        }
        return nil
    }

    var isRemoteContext: Bool { activeRemoteDevice != nil }

    var selectedDeviceID: String {
        activeRemoteDevice?.id
            ?? devices.first(where: { $0.kind != "remote" })?.id
            ?? "local"
    }

    var activeContextLabel: String? {
        activeRemoteDevice?.label
    }

    var focusedPaneID: String? {
        if isRemoteContext {
            return remote.navigation?.focusedPaneID
        }
        return core.snapshot?.paneLayout?.focusedPaneID
            ?? core.snapshot?.terminal.paneID
    }

    func paneMetadata(for paneID: String) -> CorePaneSnapshot? {
        focusedPanes.first(where: { $0.id == paneID })
            ?? workspaces
                .lazy
                .flatMap(\.checkouts)
                .flatMap(\.tabs)
                .flatMap(\.panes)
                .first(where: { $0.id == paneID })
    }

    func paneStatus(for paneID: String) -> String {
        if isRemoteContext {
            return paneMetadata(for: paneID)?.state ?? "unavailable"
        }
        return core.snapshot?.terminal.panes.first(where: { $0.paneID == paneID })?.closed == true
            ? "closed"
            : "attached"
    }

    func focusPane(_ paneID: String) {
        if isRemoteContext {
            guard let workspace = focusedWorkspace,
                  let checkout = focusedCheckout
            else {
                interactionNotice = "The remote pane cannot be focused because its workspace selection is unavailable."
                return
            }
            remote.focus(
                workspaceID: workspace.id,
                checkoutID: checkout.id,
                paneID: paneID
            )
            if agents.contains(where: { $0.paneID == paneID }) {
                remote.perform(.focusAgent(paneID: paneID))
            }
            HideLaunchTrace.mark("pane.selection", detail: "remote_\(paneID)")
        } else {
            core.focusPane(paneID)
            HideLaunchTrace.mark("pane.selection", detail: "local_\(paneID)")
        }
        focus(.terminal)
    }

    func openTerminalLink(_ rawValue: String, paneID: String) {
        switch TerminalLinkResolver.parse(rawValue) {
        case .web(let url):
            guard NSWorkspace.shared.open(url) else {
                interactionNotice = "The default browser could not open this terminal URL."
                HideLaunchTrace.mark("terminal.link.failed", detail: "browser_open")
                return
            }
            HideLaunchTrace.mark("terminal.link.opened", detail: "web")
        case .file(let path, _, _):
            if isRemoteContext {
                interactionNotice = "\(path) belongs to the remote device. Remote Workbench preview is not available in the current read-only snapshot contract."
                focus(.workbench)
                HideLaunchTrace.mark("terminal.link.failed", detail: "remote_file_contract")
                return
            }
            let paneCWD = paneMetadata(for: paneID)?.cwd ?? ""
            let checkoutRoot = focusedCheckout.map {
                URL(fileURLWithPath: $0.path, isDirectory: true)
            }
            switch TerminalLinkResolver.resolveLocalFile(
                path: path,
                paneCWD: paneCWD,
                checkoutRoot: checkoutRoot
            ) {
            case .file(let url):
                core.openFile(url)
                focus(.workbench)
                interactionNotice = nil
                HideLaunchTrace.mark("terminal.link.opened", detail: "workbench_file")
            case .failure(let message):
                interactionNotice = message
                HideLaunchTrace.mark("terminal.link.failed", detail: "local_file_resolution")
            }
        case .invalid(let message):
            interactionNotice = message
            HideLaunchTrace.mark("terminal.link.failed", detail: "invalid_target")
        }
    }

    var herdrIsConnected: Bool {
        if isRemoteContext {
            return remote.phase == .ready
        }
        return core.snapshot?.status.herdr.state == "connected"
    }

    func openNewWorkspace() {
        showNewWorkspace = true
        interactionNotice = nil
    }

    func openNewAgent(checkoutID: String? = nil) {
        selectedAgentCheckoutID = checkoutID ?? focusedCheckout?.id
        selectedAgentDeviceID = core.snapshot?.navigator.focusedDeviceID ?? "local"
        showNewAgent = true
        interactionNotice = nil
    }

    func openSearch() {
        showSearch = true
        interactionNotice = nil
    }

    func selectCheckout(_ checkout: CoreCheckoutSnapshot) {
        let action = CheckoutSelectionPolicy.action(for: checkout)
        HideLaunchTrace.mark(
            "checkout.selection",
            detail: "\(isRemoteContext ? "remote" : "local")_\(checkout.id)_\(action == .startTerminal ? "start_terminal" : "focus_existing")"
        )
        if isRemoteContext {
            remote.focus(workspaceID: checkout.workspaceID, checkoutID: checkout.id)
            focus(.terminal)
            if action == .startTerminal {
                remote.startTerminal(checkout: checkout)
            } else {
                remote.focusWorkspace(projectedID: checkout.workspaceID)
            }
            return
        }
        core.focusCheckout(workspaceID: checkout.workspaceID, checkoutID: checkout.id)
        focus(.terminal)
        switch action {
        case .focusExisting:
            checkoutStartState = .idle
        case .startTerminal:
            startTerminal(for: checkout)
        }
    }

    func selectDevice(_ device: CoreDeviceSnapshot) {
        if device.kind == "remote" {
            guard let alias = device.sshAlias?.trimmingCharacters(in: .whitespacesAndNewlines),
                  !alias.isEmpty
            else {
                interactionNotice = "\(device.label) is registered as remote but has no SSH alias. Repair the device in Settings before retrying."
                HideLaunchTrace.mark("device.selection.failed", detail: "remote_alias_missing_\(device.id)")
                return
            }
            activeRemoteDevice = device
            lastRemoteDevice = device
            checkoutStartState = .idle
            core.selectCommandDevice(id: device.id, label: device.label, isRemote: true)
            HideLaunchTrace.mark("device.selection", detail: "remote_\(device.id)")
            core.focusDevice(device.id)
            remote.refresh(targetID: device.id, label: device.label, sshAlias: alias)
            focus(.terminal)
            return
        }
        activeRemoteDevice = nil
        remote.clearNavigation()
        checkoutStartState = .idle
        core.selectCommandDevice(id: device.id, label: device.label, isRemote: false)
        HideLaunchTrace.mark("device.selection", detail: "local_\(device.id)")
        core.focusDevice(device.id)
    }

    func selectAgent(_ agent: SidebarAgent) {
        guard let workspace = workspaces.first(where: { $0.label == agent.workspaceLabel }),
              let checkout = workspace.checkouts.first(where: { checkout in
                  checkout.tabs.contains(where: { tab in tab.panes.contains(where: { $0.id == agent.paneID }) })
              }) ?? workspace.checkouts.first
        else {
            interactionNotice = "The selected agent is not attached to a known checkout."
            return
        }
        if isRemoteContext {
            remote.focus(workspaceID: workspace.id, checkoutID: checkout.id, paneID: agent.paneID)
            remote.perform(.focusAgent(paneID: agent.paneID))
        } else {
            core.focusCheckout(workspaceID: workspace.id, checkoutID: checkout.id)
            core.focusPane(agent.paneID)
        }
        focus(.terminal)
    }

    func addWorkspace(path: URL, label: String, initializeGit: Bool) {
        guard path.isFileURL else {
            interactionNotice = "Choose a local folder before adding the workspace."
            return
        }
        core.createWorkspace(path: path, label: label, initializeGit: initializeGit)
        showNewWorkspace = false
        interactionNotice = "Workspace registration requested. Files stay where they are."
    }

    func requestRemoveWorkspace(_ workspace: CoreWorkspaceSnapshot) {
        workspaceToRemove = workspace
    }

    func confirmRemoveWorkspace() {
        guard let workspace = workspaceToRemove else { return }
        core.removeWorkspace(workspace.id)
        workspaceToRemove = nil
        interactionNotice = "Workspace removed from Hide. Its folder, repository, and worktrees were not changed."
    }

    func addTab() {
        guard let workspace = focusedWorkspace else {
            interactionNotice = "Create or register a workspace before adding a tab."
            return
        }
        if isRemoteContext, let checkout = focusedCheckout {
            remote.createTab(checkout: checkout)
        } else {
            core.createTab(workspaceID: workspace.id, checkoutID: focusedCheckout?.id)
        }
    }

    func focusTab(_ tab: CoreTabSnapshot) {
        guard let tabID = tab.id,
              let workspace = focusedWorkspace,
              let checkout = focusedCheckout
        else {
            interactionNotice = "The selected tab has no routable workspace context. No tab focus command was sent."
            return
        }
        if isRemoteContext {
            remote.focus(
                workspaceID: workspace.id,
                checkoutID: checkout.id,
                tabID: tabID,
                paneID: tab.panes.first?.id
            )
            remote.perform(.focusTab(tabID: tabID))
        } else {
            core.focusTab(
                workspaceID: workspace.id,
                checkoutID: checkout.id,
                tabID: tabID
            )
        }
        focus(.terminal)
    }

    func startAgent() {
        guard let checkout = selectedAgentCheckout else {
            interactionNotice = "Choose a checkout before starting an agent."
            return
        }
        guard selectedAgentDeviceID == "local" else {
            interactionNotice = "Remote agent start is delegated to the remote Herdr session in v1. Connect that device first."
            return
        }
        core.startAgent(
            agent: selectedAgentKind,
            checkoutPath: checkout.path,
            bypassWarnings: agentBypassWarnings
        )
        showNewAgent = false
        interactionNotice = "Agent start requested. Hide does not handle credentials."
    }

    func updatePreferences(accentHex: String? = nil, fontSize: Double? = nil, bypassWarnings: Bool? = nil) {
        core.persistUIState(
            accentHex: accentHex,
            fontSize: fontSize,
            bypassWarnings: bypassWarnings
        )
    }

    func clearInteractionNotice() {
        interactionNotice = nil
    }

    func addDevice(label: String, alias: String) {
        let trimmedAlias = alias.trimmingCharacters(in: .whitespacesAndNewlines)
        let id = "device:\(trimmedAlias.lowercased().replacingOccurrences(of: "[^a-z0-9]+", with: "-", options: .regularExpression))"
        core.registerDevice(
            id: id,
            label: label.trimmingCharacters(in: .whitespacesAndNewlines),
            sshAlias: trimmedAlias
        )
        interactionNotice = "Device registration requested. Hide will use your existing SSH environment."
    }

    func removeDevice(_ device: CoreDeviceSnapshot) {
        core.removeDevice(device.id)
        interactionNotice = "Device removed from Hide. The remote host and its sessions were not changed."
    }

    func testDevice(_ device: CoreDeviceSnapshot) {
        core.testDevice(device.id)
        lastRemoteDevice = device
        if let alias = device.sshAlias {
            remote.refresh(targetID: device.id, label: device.label, sshAlias: alias)
        }
        interactionNotice = "Connection test requested for \(device.label). Authentication remains owned by SSH."
    }

    func retryRemote() {
        guard let device = lastRemoteDevice ?? devices.first(where: { $0.kind == "remote" }),
              let alias = device.sshAlias else {
            interactionNotice = "Add an SSH device before retrying a remote connection."
            return
        }
        remote.refresh(targetID: device.id, label: device.label, sshAlias: alias)
    }

    private func startTerminal(for checkout: CoreCheckoutSnapshot) {
        guard pendingCheckoutStarts.insert(checkout.id).inserted else {
            checkoutStartState = .starting
            interactionNotice = "A terminal is already starting for this checkout."
            return
        }
        guard let runtime = core.runtimeSelection else {
            pendingCheckoutStarts.remove(checkout.id)
            checkoutStartState = .failed("The bundled Herdr runtime is still starting. Wait for Herdr status, then select this checkout again.")
            interactionNotice = "The bundled Herdr runtime is not ready yet. Select this checkout again when Herdr status is available."
            return
        }
        checkoutStartState = .starting
        let checkoutID = checkout.id
        let checkoutPath = checkout.path
        let checkoutLabel = checkout.label
        Task { @MainActor [weak self] in
            let result = await Task.detached(priority: .userInitiated) {
                HerdrTerminalLauncher.launch(
                    herdrPath: runtime.path,
                    checkoutPath: checkoutPath,
                    checkoutLabel: checkoutLabel
                )
            }.value
            guard let self else { return }
            pendingCheckoutStarts.remove(checkoutID)
            if result.succeeded, let paneID = result.paneID {
                // Keep Herdr's global focus untouched. The returned pane is
                // the local projection anchor until the next session poll
                // supplies its authoritative layout.
                core.persistUIState(
                    selectedPaneID: paneID,
                    focusedCheckoutID: checkoutID
                )
                checkoutStartState = .started
                HideLaunchTrace.mark("checkout.terminal_start.ready", detail: "\(checkoutID)_\(paneID)")
            } else {
                let message = result.succeeded
                    ? "Herdr created a workspace but did not return its terminal pane. Check Herdr status and retry."
                    : result.message
                checkoutStartState = .failed(message)
                interactionNotice = message
                HideLaunchTrace.mark("checkout.terminal_start.failed", detail: checkoutID)
            }
        }
    }

    var selectedAgentCheckout: CoreCheckoutSnapshot? {
        guard let selectedAgentCheckoutID else { return focusedCheckout }
        return workspaces.lazy.compactMap { workspace in
            workspace.checkouts.first(where: { $0.id == selectedAgentCheckoutID })
        }.first
    }

    func focus(_ surface: ShellSurface) {
        activeSurface = surface
    }

    private enum PaneCommandRoute {
        case local
        case remote(paneID: String)
        case unavailable(String)
    }

    private enum PaneCloseTarget {
        case local(paneID: String)
        case remote(paneID: String)

        var paneID: String {
            switch self {
            case .local(let paneID), .remote(let paneID): paneID
            }
        }
    }

    private var paneCommandRoute: PaneCommandRoute {
        guard let device = activeRemoteDevice else { return .local }
        guard remote.phase == .ready else {
            return .unavailable("\(device.label) is not ready. No pane command was sent to either device.")
        }
        guard let navigation = remote.navigation,
              navigation.deviceID == device.id
        else {
            return .unavailable("\(device.label) has no matching navigation snapshot. No pane command was sent to either device.")
        }
        guard let paneID = navigation.focusedPaneID else {
            return .unavailable("Select a pane on \(device.label) before using a pane command. No local pane was changed.")
        }
        return .remote(paneID: paneID)
    }

    private func splitCurrentPane(_ direction: PaneSplitDirection) {
        core.splitCurrentPane(direction: direction)
        focus(.terminal)
    }

    private func toggleCurrentPaneZoom() {
        core.toggleCurrentPaneZoom()
        focus(.terminal)
    }

    private func closeCurrentPane(target closeTarget: PaneCloseTarget) {
        let paneID = closeTarget.paneID
        guard paneMetadata(for: paneID) != nil else {
            consequenceResult = "Select a terminal pane before closing."
            return
        }
        let agent = agents.first { $0.paneID == paneID }
        let target = DestructiveTarget(
            id: paneID,
            label: paneID,
            state: agent?.state ?? "idle",
            summary: agent?.summary ?? "No working or attention state is reported for this pane."
        )
        let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
        consequenceResult = nil
        if notice.requiresConfirmation {
            pendingPaneCloseTarget = closeTarget
            consequenceNotice = notice
        } else {
            pendingPaneCloseTarget = nil
            executePaneClose(closeTarget, confirmed: false)
            consequenceResult = "Close requested for idle pane \(paneID)."
        }
        focus(.terminal)
    }

    func performPaneCommand(_ command: PaneCommand) {
        switch paneCommandRoute {
        case .unavailable(let message):
            interactionNotice = message
            HideLaunchTrace.mark(
                "pane.command.blocked",
                detail: "command=\(command.rawValue) device_id=\(selectedDeviceID)"
            )
        case .local:
            switch command {
            case .splitRight: splitCurrentPane(.right)
            case .splitDown: splitCurrentPane(.down)
            case .toggleZoom: toggleCurrentPaneZoom()
            case .closePane:
                guard let paneID = focusedPaneID else {
                    consequenceResult = "Select a terminal pane before closing."
                    return
                }
                closeCurrentPane(target: .local(paneID: paneID))
            }
        case .remote(let paneID):
            switch command {
            case .splitRight: remote.perform(.split(paneID: paneID, direction: .right))
            case .splitDown: remote.perform(.split(paneID: paneID, direction: .down))
            case .toggleZoom: remote.perform(.toggleZoom(paneID: paneID))
            case .closePane: closeCurrentPane(target: .remote(paneID: paneID))
            }
            focus(.terminal)
        }
    }

    func shortcut(for command: PaneCommand) -> PaneShortcut {
        paneShortcuts[command] ?? command.defaultShortcut
    }

    func updateShortcut(_ command: PaneCommand, raw: String) {
        switch PaneShortcutPolicy.updating(command: command, raw: raw, current: paneShortcuts) {
        case let .success(updated):
            paneShortcuts = updated
            shortcutErrors[command] = nil
            shortcutDiagnostic = "Pane menu shortcuts were updated and saved."
            core.persistUIState(
                shortcutBindings: Dictionary(uniqueKeysWithValues: updated.map {
                    ($0.key.rawValue, $0.value.canonical)
                })
            )
        case let .failure(error):
            shortcutErrors[command] = error.localizedDescription
            shortcutDiagnostic = "The invalid shortcut was not saved."
        }
    }

    /// Stores the pet's global shortcut. The registrar reports back whether
    /// macOS accepted it, so a conflict is visible rather than a shortcut
    /// that silently never fires.
    func updatePetShortcut(_ accelerator: String?) {
        core.updatePetShortcut(accelerator: accelerator, error: petShortcutRegistrar?(accelerator))
    }

    /// Set by the app delegate, which owns the Carbon registration.
    var petShortcutRegistrar: ((String?) -> String?)?

    func previewConsequence(_ kind: DestructiveTargetKind) {
        pendingPaneCloseTarget = nil
        let agents = core.snapshot?.navigator.agents ?? []
        let targets = agents.map {
            DestructiveTarget(id: $0.paneID, label: $0.workspaceLabel, state: $0.state, summary: $0.summary)
        }
        consequenceNotice = ConsequencePolicy.notice(kind: kind, targets: targets)
        consequenceResult = nil
    }

    func confirmConsequencePreview() {
        guard let consequenceNotice else { return }
        if let target = pendingPaneCloseTarget {
            let paneID = target.paneID
            executePaneClose(target, confirmed: true)
            consequenceResult = "Confirmed close requested for pane \(paneID)."
            pendingPaneCloseTarget = nil
        } else {
            consequenceResult = consequenceNotice.requiresConfirmation
                ? "Confirmation recorded for the prefixed verification preview. No user resource was changed."
                : "Idle close requires no confirmation. No user resource was changed in this preview."
        }
        self.consequenceNotice = nil
    }

    func cancelConsequencePreview() {
        consequenceResult = "Cancelled before any process or checkout was affected."
        pendingPaneCloseTarget = nil
        consequenceNotice = nil
    }

    private func executePaneClose(_ target: PaneCloseTarget, confirmed: Bool) {
        switch target {
        case .local(let paneID):
            core.closePane(paneID, confirmed: confirmed)
        case .remote(let paneID):
            remote.perform(.close(paneID: paneID))
        }
    }

    private static func uiStateDiagnostic(_ snapshot: CoreSnapshot?) -> String? {
        guard let diagnostic = snapshot?.status.diagnostics.last(where: {
            $0.kind == "ui_state.missing" || $0.kind == "ui_state.corrupt"
        }) else { return nil }
        return switch diagnostic.kind {
        case "ui_state.missing": "No shortcut state file was found. Default pane shortcuts are active."
        default: "The shortcut state file was corrupt. Default pane shortcuts are active."
        }
    }
}
