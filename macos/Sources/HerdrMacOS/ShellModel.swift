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

enum PaneSplitDirection: String, Decodable, Equatable {
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
    private var pendingPaneCloseID: String?
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

    var isRemoteContext: Bool { activeRemoteDevice != nil }

    var activeContextLabel: String? {
        activeRemoteDevice?.label
    }

    var focusedPaneID: String? {
        if isRemoteContext {
            return remote.navigation?.focusedPaneID
        }
        return core.snapshot?.terminal.paneID
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
        if isRemoteContext {
            remote.focus(workspaceID: checkout.workspaceID, checkoutID: checkout.id)
            focus(.terminal)
            if CheckoutSelectionPolicy.action(for: checkout) == .startTerminal {
                remote.startTerminal(checkout: checkout)
            }
            return
        }
        core.focusCheckout(workspaceID: checkout.workspaceID, checkoutID: checkout.id)
        focus(.terminal)
        switch CheckoutSelectionPolicy.action(for: checkout) {
        case .focusExisting:
            checkoutStartState = .idle
        case .startTerminal:
            startTerminal(for: checkout)
        }
    }

    func selectDevice(_ device: CoreDeviceSnapshot) {
        if device.kind == "remote", let alias = device.sshAlias {
            activeRemoteDevice = device
            lastRemoteDevice = device
            checkoutStartState = .idle
            core.focusDevice(device.id)
            remote.refresh(targetID: device.id, label: device.label, sshAlias: alias)
            focus(.terminal)
            return
        }
        activeRemoteDevice = nil
        remote.clearNavigation()
        checkoutStartState = .idle
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
            remote.startTerminal(checkout: checkout)
        } else {
            core.createTab(workspaceID: workspace.id, checkoutID: focusedCheckout?.id)
        }
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
            } else {
                let message = result.succeeded
                    ? "Herdr created a workspace but did not return its terminal pane. Check Herdr status and retry."
                    : result.message
                checkoutStartState = .failed(message)
                interactionNotice = message
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

    func splitCurrentPane(_ direction: PaneSplitDirection) {
        core.splitCurrentPane(direction: direction)
        focus(.terminal)
    }

    func toggleCurrentPaneZoom() {
        core.toggleCurrentPaneZoom()
        focus(.terminal)
    }

    func closeCurrentPane() {
        guard let paneID = core.snapshot?.terminal.paneID else {
            consequenceResult = "Select a terminal pane before closing."
            return
        }
        let agent = core.snapshot?.navigator.agents.first { $0.paneID == paneID }
        let target = DestructiveTarget(
            id: paneID,
            label: paneID,
            state: agent?.state ?? "idle",
            summary: agent?.summary ?? "No working or attention state is reported for this pane."
        )
        let notice = ConsequencePolicy.notice(kind: .pane, targets: [target])
        consequenceResult = nil
        if notice.requiresConfirmation {
            pendingPaneCloseID = paneID
            consequenceNotice = notice
        } else {
            pendingPaneCloseID = nil
            core.closePane(paneID, confirmed: false)
            consequenceResult = "Close requested for idle pane \(paneID)."
        }
        focus(.terminal)
    }

    func performPaneCommand(_ command: PaneCommand) {
        switch command {
        case .splitRight: splitCurrentPane(.right)
        case .splitDown: splitCurrentPane(.down)
        case .toggleZoom: toggleCurrentPaneZoom()
        case .closePane: closeCurrentPane()
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
        pendingPaneCloseID = nil
        let agents = core.snapshot?.navigator.agents ?? []
        let targets = agents.map {
            DestructiveTarget(id: $0.paneID, label: $0.workspaceLabel, state: $0.state, summary: $0.summary)
        }
        consequenceNotice = ConsequencePolicy.notice(kind: kind, targets: targets)
        consequenceResult = nil
    }

    func confirmConsequencePreview() {
        guard let consequenceNotice else { return }
        if let paneID = pendingPaneCloseID {
            core.closePane(paneID, confirmed: true)
            consequenceResult = "Confirmed close requested for pane \(paneID)."
            pendingPaneCloseID = nil
        } else {
            consequenceResult = consequenceNotice.requiresConfirmation
                ? "Confirmation recorded for the prefixed verification preview. No user resource was changed."
                : "Idle close requires no confirmation. No user resource was changed in this preview."
        }
        self.consequenceNotice = nil
    }

    func cancelConsequencePreview() {
        consequenceResult = "Cancelled before any process or checkout was affected."
        pendingPaneCloseID = nil
        consequenceNotice = nil
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
